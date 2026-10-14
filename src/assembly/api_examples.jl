using TOML
using REPL

struct ExampleDisplay <: AbstractDisplay
    io::IO
    scope::Module
end

function Base.display(display::ExampleDisplay, value)
    show(IOContext(display.io, :limit => true, :displaysize => (24, 80),
                   :color => false, :module => display.scope), MIME"text/plain"(), value)
    println(display.io)
end

function captured(scope, code, directory; suppress=false)
    mktemp(directory) do path, io
        display = ExampleDisplay(io, scope)
        pushdisplay(display)
        failed = false
        try
            redirect_stdout(io) do
                redirect_stderr(io) do
                    try
                        value = Core.eval(scope, REPL.softscope(Meta.parseall(code)))
                        if !suppress && value !== nothing
                            Base.display(display, value)
                        end
                    catch exception
                        failed = true
                        print(io, "ERROR: ")
                        showerror(io, exception)
                        println(io)
                    end
                end
            end
        finally
            popdisplay(display)
        end
        flush(io)
        seekstart(io)
        return read(io, String), failed
    end
end

function options(header)
    parts = split(header, ';'; limit=2)
    matched = match(r"^jldoctest(?:\s+([A-Za-z_]\w*))?$", strip(parts[1]))
    isnothing(matched) && error("Unsupported jldoctest header: $header")
    settings = Dict{Symbol,Any}()
    if length(parts) == 2
        expression = Meta.parse("call(; " * parts[2] * ")")
        for keyword in expression.args[2].args
            keyword isa Expr && keyword.head == :kw || error("Unsupported doctest option.")
            name, value = keyword.args
            name in (:setup, :teardown, :output) || error("Unsupported doctest option: $name")
            haskey(settings, name) && error("Duplicate doctest option: $name")
            settings[name] = value
        end
    end
    return matched.captures[1], settings
end

function setup(scope, settings, name)
    haskey(settings, name) || return
    value = Core.eval(scope, settings[name])
    value isa Union{Expr,Symbol,QuoteNode} || error("Doctest $name must supply quoted Julia code.")
    Core.eval(scope, value)
end

function expressions(lines, first, last, offsets)
    examples = []
    if any(startswith(line, "julia>") for line in lines[first:last])
        index = first
        while index <= last
            isempty(strip(lines[index])) && (index += 1; continue)
            startswith(lines[index], "julia> ") || error("Unsupported Julia REPL doctest syntax.")
            start = index
            source = String[lines[index][8:end]]
            index += 1
            while index <= last && startswith(lines[index], "       ")
                push!(source, lines[index][8:end])
                index += 1
            end
            expected = String[]
            while index <= last && !startswith(lines[index], "julia>")
                push!(expected, lines[index])
                index += 1
            end
            push!(examples, (join(source), join(expected), offsets[start], offsets[index], true))
        end
    else
        marker = findall(line -> strip(line) == "# output", lines[first:last])
        length(marker) == 1 || error("A script doctest requires exactly one # output line.")
        index = first + only(marker) - 1
        push!(examples, (join(lines[first:index-1]), join(lines[index+1:last]),
                         offsets[first], offsets[last+1], false))
    end
    examples
end

function run(request_path, output_path)
    request = TOML.parsefile(request_path)
    root = request["package_root"]
    project = TOML.parsefile(joinpath(root, "Project.toml"))
    project["name"] == request["package_name"] || error("Checkout package identity mismatch.")
    project["version"] == request["package_version"] || error("Checkout package version mismatch.")
    loaded = Base.require(Base.PkgId(Base.UUID(project["uuid"]), project["name"]))
    realpath(pathof(loaded)) == realpath(joinpath(root, "src", project["name"] * ".jl")) ||
        error("Loaded package does not match the supplied checkout.")
    string(Base.pkgversion(loaded)) == project["version"] ||
        error("Loaded package version conflicts with checkout metadata.")
    source = request["source"]
    occursin(r"(?m)^\s*[`~]{3,}@meta", source) &&
        error("Doctest @meta setup is unsupported; use block-level setup and teardown.")
    lines = collect(eachline(IOBuffer(source); keep=true))
    offsets = [0; cumsum(ncodeunits.(lines))]
    outcomes = []
    scopes = Dict{String,Module}()
    index = 1
    while index <= length(lines)
        fence = match(r"^\s*(`{3,}|~{3,})(.*)$", lines[index])
        if isnothing(fence)
            index += 1
            continue
        end
        delimiter, header = fence.captures
        start_line = index + 1
        index = start_line
        closing = Regex("^\\s*" * first(delimiter) * "{" * string(length(delimiter)) * ",}\\s*\$")
        while index <= length(lines) && isnothing(match(closing, lines[index]))
            index += 1
        end
        if startswith(strip(header), "@example") || startswith(strip(header), "@repl")
            error("Unsupported executable Julia fence: $(strip(header))")
        end
        if !startswith(strip(header), "jldoctest")
            index += 1
            continue
        end
        index <= length(lines) || error("Unclosed executable Julia doctest fence.")
        label, settings = options(header)
        scope = isnothing(label) ? Module(:DiplodocusExample) : get!(scopes, label) do
            Module(:DiplodocusExample)
        end
        Core.eval(scope, Meta.parse("using " * project["name"]))
        if haskey(settings, :output)
            value = Core.eval(scope, settings[:output])
            value isa Bool || error("Doctest output must be Boolean.")
            settings[:output] = value
        end
        setup(scope, settings, :setup)
        try
            for (code, expected, start, stop, repl) in expressions(lines, start_line, index-1, offsets)
                actual, failed = captured(scope, code, dirname(output_path);
                                          suppress=repl && REPL.ends_with_semicolon(code))
                wanted = strip(expected, ['\r', '\n'])
                got = strip(actual, ['\r', '\n'])
                if failed
                    prefix = first(split(wanted, "\n[...]"; limit=2))
                    startswith(wanted, "ERROR:") && startswith(got, prefix) ||
                        error("Julia doctest mismatch.\nSource:\n$code\nExpected:\n$wanted\nActual:\n$got")
                elseif got != wanted
                    error("Julia doctest mismatch.\nSource:\n$code\nExpected:\n$wanted\nActual:\n$got")
                end
                outcome = Dict("span" => Dict("start" => start, "end" => stop),
                    "source" => code, "expected" => expected, "actual" => actual,
                    "outcome" => failed ? "expected-error" : "executed",
                    "options" => Dict(string(k) => string(v) for (k,v) in settings))
                push!(outcomes, outcome)
            end
        finally
            setup(scope, settings, :teardown)
        end
        index += 1
    end
    dependencies = Dict{String,String}()
    for (id, dependency) in Base.loaded_modules
        version = Base.pkgversion(dependency)
        isnothing(version) || (dependencies[string(id.uuid)] = string(version))
    end
    result = Dict("runtime_version" => string(VERSION), "package_name" => project["name"],
                  "package_version" => project["version"], "dependencies" => dependencies,
                  "examples" => outcomes)
    open(output_path, "w") do io
        TOML.print(io, result; sorted=true)
    end
end

run(ARGS...)
