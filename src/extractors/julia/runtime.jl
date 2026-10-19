using TOML

# Loading by package identity lets Julia resolve dependencies in the caller's
# prepared environment. Verify the checkout before reflecting on its bindings.
function observe_checkout(package_path, entry_path, output_path, reflect="true")
    project = TOML.parsefile(joinpath(package_path, "Project.toml"))
    identity = Base.PkgId(Base.UUID(project["uuid"]), project["name"])
    loaded = Base.require(identity)
    Base.PkgId(loaded) == identity || error("Loaded package identity conflicts with Project.toml.")
    nameof(loaded) == Symbol(project["name"]) ||
        error("Loaded package name conflicts with Project.toml.")
    version = Base.pkgversion(loaded)
    if haskey(project, "version")
        version == VersionNumber(project["version"]) ||
            error("Loaded package version conflicts with Project.toml.")
    end
    loaded_entry = pathof(loaded)
    isnothing(loaded_entry) && error("Loaded package has no source entry.")
    realpath(loaded_entry) == realpath(entry_path) ||
        error("Loaded package does not match the supplied checkout.")

    bindings = []
    for name in (reflect == "true" ? sort!(names(loaded; all=false, imported=false); by=string) : Symbol[])
        isdefined(loaded, name) || error("Public binding is undefined: $name")
        binding = getfield(loaded, name)
        (binding isa DataType || binding isa UnionAll) || continue
        value = Base.unwrap_unionall(binding)
        parentmodule(value) === loaded || continue
        push!(bindings, Dict(
            "name" => string(name),
            "qualified_name" => string(loaded, '.', name),
            "supertype" => string(supertype(value)),
            "abstract" => isabstracttype(value),
            "mutable" => ismutabletype(value),
            "parameters" => [string(parameter) for parameter in value.parameters],
            "fields" => [Dict("name" => string(field), "type" => string(fieldtype(value, field)))
                         for field in (isabstracttype(value) ? () : fieldnames(value))],
        ))
    end
    dependencies = Dict{String,String}()
    for (id, dependency) in Base.loaded_modules
        version = Base.pkgversion(dependency)
        isnothing(version) || (dependencies[string(id.uuid)] = string(version))
    end
    result = Dict(
        "policy" => "julia-runtime-v1",
        "julia_version" => string(VERSION),
        "package_name" => string(nameof(loaded)),
        "package_uuid" => string(Base.PkgId(loaded).uuid),
        "package_version" => string(Base.pkgversion(loaded)),
        "dependencies" => dependencies,
        "types" => bindings,
    )
    open(output_path, "w") do io
        TOML.print(io, result; sorted=true)
    end
end

if abspath(PROGRAM_FILE) == @__FILE__
    observe_checkout(ARGS...)
end
