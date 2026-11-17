using TOML
using Libdl
using Artifacts
import Pkg

function observe_environment(output, package_root)
    roots = String[]
    artifact_files = String[]
    projects = String[]
    eligible = Sys.islinux()
    function environment(project)
        directory = isfile(project) ? dirname(project) : project
        push!(artifact_files, joinpath(directory, "Artifacts.toml"))
        for name in ("Project.toml", "JuliaProject.toml", "Manifest.toml", "JuliaManifest.toml",
                     "Manifest-v$(VERSION.major).$(VERSION.minor).toml", "LocalPreferences.toml")
            push!(roots, abspath(directory, name))
        end
        if isfile(project)
            push!(projects, realpath(project))
        end
    end
    active = Base.active_project()
    isnothing(active) && error("No prepared Julia project")
    environment(active)
    isdir(package_root) || append!(roots, [joinpath(dirname(active), name) for name in ("src", "ext", "deps", "data", "Artifacts.toml")])
    expanded = Base.load_path()
    for path in expanded
        path == Sys.STDLIB ? push!(roots, abspath(path)) : environment(path)
    end
    for depot in DEPOT_PATH
        overrides = abspath(depot, "artifacts", "Overrides.toml")
        push!(roots, overrides)
        if isfile(overrides)
            function retain_overrides(value)
                if value isa AbstractDict
                    foreach(retain_overrides, values(value))
                elseif value isa AbstractString && isabspath(value)
                    push!(roots, value)
                elseif value isa AbstractString && occursin(r"^[0-9a-f]{40}$", value)
                    append!(roots, [abspath(other, "artifacts", value) for other in DEPOT_PATH])
                end
            end
            retain_overrides(TOML.parsefile(overrides))
        end
        environment(abspath(depot, "environments", "v$(VERSION.major).$(VERSION.minor)", "Project.toml"))
    end
    for path in LOAD_PATH
        if !startswith(path, '@')
            environment(abspath(expanduser(path)))
        end
    end
    dependencies = Any[]
    for project in unique(projects)
        metadata = TOML.parsefile(project)
        if !isempty(get(metadata, "deps", Dict())) || isfile(joinpath(dirname(project), "Manifest.toml"))
            Base.invokelatest(Pkg.activate, dirname(project); io=devnull)
            for (id, dependency) in sort!(collect(Base.invokelatest(Pkg.dependencies)); by=entry -> string(first(entry)))
                source = abspath(dependency.source)
                isdir(source) || (eligible = false)
                realpath(source) == realpath(package_root) || push!(roots, source)
                push!(artifact_files, joinpath(source, "Artifacts.toml"))
                for depot in DEPOT_PATH
                    marker = joinpath(depot, "packages")
                    if startswith(source, string(marker, Base.Filesystem.path_separator))
                        suffix = relpath(source, marker)
                        append!(roots, [abspath(other, "packages", suffix) for other in DEPOT_PATH])
                    end
                end
                push!(dependencies, (string(id), dependency.name, string(dependency.version), source))
            end
        end
    end
    for path in unique(artifact_files)
        push!(roots, path)
        if isfile(path)
            for name in keys(TOML.parsefile(path))
                hash = artifact_hash(name, path)
                if isnothing(hash)
                    eligible = false
                else
                    append!(roots, [abspath(depot, "artifacts", string(hash)) for depot in DEPOT_PATH])
                    push!(roots, artifact_path(hash))
                end
            end
        end
    end
    push!(roots, realpath(joinpath(Sys.BINDIR, "julia")))
    push!(roots, realpath(Sys.STDLIB))
    maps = "/proc/self/maps"
    if isfile(maps)
        for line in eachline(maps)
            fields = split(line; limit=6)
            path = length(fields) == 6 ? strip(fields[6]) : ""
            if startswith(path, '/')
                any(depot -> startswith(path, abspath(depot, "compiled") * "/") ||
                             startswith(path, abspath(depot, "logs") * "/"), DEPOT_PATH) && continue
                isfile(path) ? push!(roots, realpath(path)) : (eligible = false)
            end
        end
    else
        eligible = false
    end
    options = Base.JLOptions()
    semantics = [getproperty(options, name) for name in
                 (:opt_level, :check_bounds, :can_inline, :fast_math, :use_sysimage_native_code, :use_compiled_modules)]
    identity = repr((string(VERSION), Sys.CPU_NAME, LOAD_PATH, expanded, DEPOT_PATH, dependencies, semantics))
    open(output, "w") do io
        TOML.print(io, Dict("eligible" => eligible, "identity" => identity, "roots" => sort!(unique(roots))); sorted=true)
    end
end

function write_completion(output)
    roots = String[]
    eligible = Sys.islinux()
    for (_, loaded) in Base.loaded_modules
        path = pathof(loaded)
        isnothing(path) || push!(roots, abspath(path))
    end
    if isfile("/proc/self/maps")
        for line in eachline("/proc/self/maps")
            fields = split(line; limit=6)
            path = length(fields) == 6 ? strip(fields[6]) : ""
            if startswith(path, '/')
                any(depot -> startswith(path, abspath(depot, "compiled") * "/") ||
                             startswith(path, abspath(depot, "logs") * "/"), DEPOT_PATH) && continue
                isfile(path) ? push!(roots, realpath(path)) : (eligible = false)
            end
        end
    else
        eligible = false
    end
    open(output, "w") do io
        TOML.print(io, Dict("eligible" => eligible, "roots" => sort!(unique(roots))); sorted=true)
    end
end

if abspath(PROGRAM_FILE) == @__FILE__
    try
        observe_environment(ARGS...)
    catch error
        open(ARGS[1], "w") do io
            TOML.print(io, Dict("eligible" => false, "identity" => "", "roots" => String[], "reason" => sprint(showerror, error)))
        end
    end
end
