using Test, TOML

include("../src/extractors/julia/runtime.jl")

@testset "Native Julia checkout export" begin
    mktempdir() do package
        write(joinpath(package, "Project.toml"), """
        name = "RuntimeExceptions"
        uuid = "12345678-1234-1234-1234-123456789abc"
        version = "1.2.3"
        """)
        mkpath(joinpath(package, "src"))
        entry = joinpath(package, "src", "RuntimeExceptions.jl")
        write(entry, """
        module RuntimeExceptions
        abstract type PackageError <: Exception end
        macro define_error(name)
            quote
                struct \$(esc(name)) <: PackageError
                    message::String
                end
            end
        end
        """ * join(["@define_error Error$i\nexport Error$i\n" for i in 1:15]) * """
        export PackageError, Container, generated, optional
        const topic = "prose"
        function generated end
        @doc "Generic \$(topic)." generated
        macro api()
            quote
                "Method prose."
                $(esc(:generated))(value::Int) = value
            end
        end
        @api
        include(joinpath(@__DIR__, "mutate.jl"))
        struct Container{T}
            value::T
            Container(x::T) where T = new{T}(x)
            Container{T}(x::T) where T = new{T}(x)
        end
        @doc "Use the parameterized constructor." Container{T}(x::T) where T
        "Use the supplied number or the default."
        optional(x::T=1) where {T<:Number} = x
        @eval const Alias = Container
        @eval export Alias
        module Values
        const ORIGINAL = 3
        end
        module Reexported
            using ..Values: ORIGINAL as RENAMED
            export RENAMED
        end
        using .Reexported
        export RENAMED
        module Inner
            export fit, shadowed
            fit(x) = x
            shadowed(x) = x
        end
        using .Inner
        shadowed(x) = x
        end
        """)
        write(joinpath(package, "src", "mutate.jl"),
              "generated(value::String) = value\n")
        pushfirst!(LOAD_PATH, package)
        try
            output = joinpath(package, "observation.toml")
            observe_checkout(package, entry, output)
            observed = TOML.parsefile(output)
            @test observed["package_version"] == "1.2.3"
            @test observed["policy"] == "julia-native-v1"
            @test isempty(observed["errors"])
            @test observed["complete"]
            @test Set(input["canonical"] for input in observed["inputs"]) ==
                Set(["src/RuntimeExceptions.jl", "src/mutate.jl"])
            items = Dict(item["name"] => item for item in observed["items"])
            types = Dict(name => item["type"] for (name, item) in items if item["kind"] == "type")
            @test length(types) == 17
            for i in 1:15
                @test types["Error$i"]["supertype"] == "RuntimeExceptions.PackageError"
                @test only(types["Error$i"]["fields"])["type"] == "String"
                @test types["Error$i"]["flavor"] == "struct"
            end
            @test types["PackageError"]["flavor"] == "abstract"
            @test isempty(types["PackageError"]["fields"])
            @test types["Container"]["parameters"] == ["T1"]
            @test only(items["Container"]["aliases"])["name"] == "RuntimeExceptions.Alias"
            constructors = Dict(method["selector"] => method for method in items["Container"]["methods"])
            @test isempty(constructors["Container(::T1) where {T1}"]["docs"])
            selected = constructors["Container{T1}(::T1) where {T1}"]
            @test only(selected["docs"])["text"] == "Use the parameterized constructor."
            @test length(selected["source_offsets"]) == 1
            optional = Dict(method["selector"] => method for method in items["optional"]["methods"])
            @test isempty(optional["optional()"]["docs"])
            @test only(optional["optional(::T1) where {T1<:Number}"]["docs"])["text"] ==
                "Use the supplied number or the default."
            @test only(items["fit"]["aliases"]) == Dict(
                "name" => "RuntimeExceptions.fit", "kind" => "import")
            @test isempty(items["shadowed"]["aliases"])
            @test !any(exported -> exported["module"] == "RuntimeExceptions" &&
                                   exported["name"] == "fit", observed["exports"])
            @test Set(alias["name"] for alias in items["ORIGINAL"]["aliases"]) ==
                Set(["RuntimeExceptions.RENAMED", "RuntimeExceptions.Reexported.RENAMED"])
            @test only(items["generated"]["docs"])["text"] == "Generic prose."
            methods = Dict(method["selector"] => method for method in items["generated"]["methods"])
            @test length(methods) == 2
            @test only(methods["generated(::Int$(Sys.WORD_SIZE))"]["docs"])["text"] == "Method prose."
            @test methods["generated(::String)"]["path"] == "src/mutate.jl"
            @test !occursin(package, read(output, String))
            @test observed["package_name"] == "RuntimeExceptions"
            @test observed["package_uuid"] == "12345678-1234-1234-1234-123456789abc"
            mismatch = joinpath(package, "src", "Other.jl")
            write(mismatch, "module Other end")
            rejected = joinpath(package, "rejected.toml")
            @test_throws ErrorException observe_checkout(package, mismatch, rejected)
            @test !isfile(rejected)
        finally
            popfirst!(LOAD_PATH)
        end
    end
end
