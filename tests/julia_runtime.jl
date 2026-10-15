using Test, TOML

include("../src/extractors/julia/runtime.jl")

@testset "Checkout runtime type observations" begin
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
        export PackageError, Container
        struct Container{T}
            value::T
        end
        end
        """)
        pushfirst!(LOAD_PATH, package)
        try
            output = joinpath(package, "observation.toml")
            observe_checkout(package, entry, output)
            observed = TOML.parsefile(output)
            @test observed["package_version"] == "1.2.3"
            @test observed["policy"] == "julia-runtime-v1"
            types = Dict(type["name"] => type for type in observed["types"])
            @test length(types) == 17
            for i in 1:15
                @test types["Error$i"]["supertype"] == "RuntimeExceptions.PackageError"
                @test types["Error$i"]["fields"] == [Dict("name" => "message", "type" => "String")]
                @test !types["Error$i"]["abstract"]
            end
            @test types["PackageError"]["abstract"]
            @test isempty(types["PackageError"]["fields"])
            @test types["Container"]["parameters"] == ["T"]
            @test !occursin(package, read(output, String))
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
