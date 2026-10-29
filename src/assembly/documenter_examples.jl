using TOML
using REPL

struct DocumenterDisplay <: AbstractDisplay
    outputs::Vector{Any}
    directory::String
    scope::Module
    stream::IOStream
    position::Base.RefValue{Int64}
    images::Base.RefValue{Int}
end

function collect_stream(display)
    flush(display.stream)
    seek(display.stream, display.position[])
    text = read(display.stream, String)
    display.position[] = position(display.stream)
    isempty(text) || push!(display.outputs, Dict("type" => "text", "text" => text, "stream" => true))
end

function Base.display(display::DocumenterDisplay, mime::MIME, value)
    collect_stream(display)
    media = string(mime)
    if media in ("image/png", "image/svg+xml", "image/jpeg")
        extension = Dict("image/png" => "png", "image/svg+xml" => "svg", "image/jpeg" => "jpg")[media]
        display.images[] += 1
        file = "image-$(display.images[]).$extension"
        open(joinpath(display.directory, file), "w") do io
            Base.invokelatest(show, IOContext(io, :limit => true, :color => false), mime, value)
        end
        push!(display.outputs, Dict("type" => "image", "media_type" => media, "file" => file))
    elseif media == "text/plain"
        text = sprint(context=(:limit => true, :displaysize => (24, 80), :color => false, :module => display.scope)) do io
            Base.invokelatest(show, io, mime, value)
        end
        push!(display.outputs, Dict("type" => "text", "text" => text))
    else
        error("Unsupported native Documenter display MIME: $media")
    end
    nothing
end

function Base.display(display::DocumenterDisplay, value)
    for media in ("image/png", "image/svg+xml", "image/jpeg", "text/plain")
        mime = MIME(media)
        if Base.invokelatest(showable, mime, value)
            Base.display(display, mime, value)
            return
        end
    end
    error("The native Documenter result has no supported representation.")
end

function evaluate(scope, code)
    value = nothing
    position = firstindex(code)
    while position <= ncodeunits(code)
        expression, position = Meta.parse(code, position; raise=true)
        isnothing(expression) && break
        value = Core.eval(scope, REPL.softscope(expression))
        Core.eval(scope, Expr(:(=), :ans, QuoteNode(value)))
    end
    value
end

function example(scope, code, directory)
    outputs = Any[]
    mktemp(directory) do path, io
        display = DocumenterDisplay(outputs, directory, scope, io, Ref(Int64(0)), Ref(0))
        pushdisplay(display)
        try
            value = redirect_stdout(io) do
                redirect_stderr(io) do
                    evaluate(scope, code)
                end
            end
            collect_stream(display)
            if value !== nothing
                filter!(output -> !get(output, "stream", false), outputs)
                Base.display(display, value)
            end
            for output in outputs
                delete!(output, "stream")
            end
        finally
            popdisplay(display)
        end
    end
    outputs
end

function run(request_path, output_path)
    request = TOML.parsefile(request_path)
    realpath(Base.active_project()) == realpath(request["project"]) || error("Julia did not activate the selected project.")
    if haskey(request, "package")
        package = request["package"]
        metadata = TOML.parsefile(package["metadata"])
        metadata["name"] == package["name"] && metadata["version"] == package["version"] || error("Julia checkout identity mismatch.")
        loaded = Base.require(Base.PkgId(Base.UUID(metadata["uuid"]), metadata["name"]))
        realpath(pathof(loaded)) == realpath(joinpath(package["root"], "src", metadata["name"] * ".jl")) || error("Loaded Julia package does not match the supplied checkout.")
        string(Base.pkgversion(loaded)) == metadata["version"] || error("Loaded Julia package version conflicts with checkout metadata.")
    end
    scopes = Dict{String,Module}()
    examples = Any[]
    for block in request["examples"]
        progress = joinpath(dirname(output_path), "progress")
        open(progress * ".tmp", "w") do io
            print(io, block["ordinal"])
        end
        mv(progress * ".tmp", progress; force=true)
        scope = if haskey(block, "session")
            get!(scopes, block["session"]) do
                Module(:DiplodocusExample)
            end
        else
            Module(:DiplodocusExample)
        end
        directory = joinpath(dirname(output_path), "block-$(block["ordinal"])")
        mkdir(directory)
        outputs = try
            example(scope, block["source"], directory)
        catch exception
            error("Documenter example $(block["ordinal"] + 1) failed: $(sprint(showerror, exception))")
        end
        push!(examples, Dict("ordinal" => block["ordinal"], "outputs" => outputs))
    end
    dependencies = Dict{String,String}()
    for (id, dependency) in Base.loaded_modules
        version = Base.pkgversion(dependency)
        isnothing(version) || (dependencies[string(id.uuid)] = string(version))
    end
    result = Dict("policy" => "documenter-examples-v1", "julia_version" => string(VERSION),
                  "dependencies" => dependencies, "examples" => examples)
    open(output_path, "w") do io
        TOML.print(io, result; sorted=true)
    end
end

run(ARGS...)
