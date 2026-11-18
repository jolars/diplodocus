using SHA
using TOML
using Markdown

const EXPORT_POLICY = "julia-native-v1"

# Exact Docs attachments, source receipts from the loader, and Method metadata
# need Julia internals. Keep those reads here and exercise them on every minor.
module NativeCompat
    using Base.JuliaSyntax: JuliaSyntax as JS

    documentation(mod) = Base.Docs.meta(mod; autoinit=false)
    used_modules(mod) = ccall(:jl_module_usings, Any, (Any,), mod)
    function cache_files(cache)
        header = Base.parse_cache_header(cache)[2]
        @static if VERSION < v"1.11"
            return first(header), Base.parse_cache_header(cache; srcfiles_only=true)[2][1]
        else
            return header[1], header[2]
        end
    end
    cache_source(cache, file) = Base.read_dependency_src(cache, file)
    cache_path(id) = Base.pkgorigins[id].cachepath
    public_binding(mod, name) = @static VERSION >= v"1.11" ?
        Base.ispublic(mod, name) : Base.isexported(mod, name)
    arguments(method) = Base.method_argnames(method)
    keywords(method) = Base.kwarg_decl(method)
    function binding_origin(mod, name, visited=())
        reference = GlobalRef(mod, name)
        @static if VERSION < v"1.12"
            isdefined(reference, :binding) || return reference
            binding = reference.binding
            return isdefined(binding, :owner) ? binding.owner.globalref : reference
        else
            world = Base.get_world_counter()
            binding = convert(Core.Binding, reference)
            while true
                partition = Base.lookup_binding_partition(world, binding)
                kind = Base.binding_kind(partition)
                if kind == Base.PARTITION_KIND_IMPLICIT_CONST
                    # These partitions store the imported value in place of a
                    # binding. Follow the exports that resolve to its owner.
                    ref = binding.globalref
                    ref.mod in visited && return ref
                    owner = Base.binding_module(ref.mod, ref.name)
                    owner === ref.mod && return ref
                    origins = unique(binding_origin(used, ref.name, (visited..., ref.mod))
                                     for used in used_modules(ref.mod)
                                     if Base.isexported(used, ref.name) &&
                                        Base.binding_module(used, ref.name) === owner)
                    actual = filter(origin -> origin.mod === owner, origins)
                    return length(actual) == 1 ? only(actual) : ref
                end
                Base.is_some_binding_imported(kind) || return binding.globalref
                binding = Base.partition_restriction(partition)::Core.Binding
            end
        end
    end
    function constructor_methods(type)
        signature = Tuple{Type{<:type}, Vararg{Any}}
        matches = Base._methods_by_ftype(signature, -1, Base.get_world_counter())
        return [match.method for match in matches
                if constructor_head(match.method, type) !== nothing]
    end
    function constructor_head(method, type)
        head = Base.unwrap_unionall(method.sig).parameters[1]
        head isa DataType && head.name === Base.unwrap_unionall(Type).name || return nothing
        value = only(head.parameters)
        base = Base.unwrap_unionall(value)
        base isa DataType && base.name === Base.unwrap_unionall(type).name || return nothing
        return value
    end

    function source_headers(path)
        declarations = Dict{Int,Vector{Pair{Int,Expr}}}()
        attachments = Dict{Int,Vector{Expr}}()
        tree = JS.parseall(JS.SyntaxNode, read(path, String); filename=path, ignore_errors=true)
        function walk(node)
            children = JS.children(node)
            isempty(something(children, ())) && return
            kind = string(JS.kind(node))
            line = JS.source_line(node)
            if kind in ("function", "=", "macro")
                header_node = first(children)
                header = Expr(header_node)
                header isa Expr || return
                # Use the header's byte offset, since a macro argument's
                # declaration node can start in the preceding whitespace.
                entry = (JS.first_byte(header_node) - 1) => header
                push!(get!(() -> Pair{Int,Expr}[], declarations, line), entry)
                header_line = JS.source_line(header_node)
                if header_line != line
                    push!(get!(() -> Pair{Int,Expr}[], declarations, header_line), entry)
                end
            elseif kind in ("doc", "macrocall")
                # Julia parses a literal preceding a definition as Core.@doc.
                callee = kind == "doc" ? Symbol("@doc") : macro_name(Expr(first(children)))
                if callee === Symbol("@doc")
                    target = Expr(last(children))
                    target isa Expr && push!(get!(() -> Expr[], attachments, line), target)
                    walk(last(children))
                elseif callee in (Symbol("@inline"), Symbol("@noinline"), Symbol("@propagate_inbounds"))
                    walk(last(children))
                end
            elseif kind in ("toplevel", "block", "module", "baremodule", "struct")
                foreach(walk, children)
            end
        end
        walk(tree)
        return (; declarations, attachments)
    end

    function macro_name(callee)
        Meta.isexpr(callee, :., 2) && callee.args[1] in (:Base, :Core) &&
            callee.args[2] isa QuoteNode && return callee.args[2].value
        return callee
    end
end

module_name(mod) = join(Base.fullname(mod), '.')
qualified(mod, name) = string(module_name(mod), '.', name)
sha(bytes) = bytes2hex(sha256(bytes))

function name_token(name)
    Base.isidentifier(name) && return string(name)
    # Quoted bindings remain distinct from compiler-generated names.
    return "var" * repr(String(name))
end

function type_spelling(value, variables)
    if value isa TypeVar
        return variables[value]
    elseif value === Union{}
        return "Union{}"
    elseif value isa Union
        branches = sort!([type_spelling(branch, variables) for branch in Base.uniontypes(value)])
        return "Union{" * join(branches, ", ") * "}"
    elseif value isa UnionAll
        body, constraints, scope = type_variables(value, variables)
        return "(" * type_spelling(body, scope) * where_clause(constraints) * ")"
    elseif Base.isvarargtype(value)
        isdefined(value, :T) || return "Vararg"
        parameters = [type_spelling(value.T, variables)]
        isdefined(value, :N) && push!(parameters, type_spelling(value.N, variables))
        return "Vararg{" * join(parameters, ", ") * "}"
    elseif value isa DataType
        owner = parentmodule(value)
        if value <: Function && Base.issingletontype(value)
            singleton = value.instance
            name = nameof(singleton)
            isdefined(owner, name) && getfield(owner, name) === singleton ||
                error("A Julia dispatch type refers to an anonymous function.")
            return "typeof(" * qualified(owner, name_token(name)) * ")"
        end
        head = owner === Core || owner === Base ? name_token(nameof(value)) :
               qualified(owner, name_token(nameof(value)))
        value === Base.unwrap_unionall(value.name.wrapper) && return head
        isempty(value.parameters) && return head
        return head * "{" * join([type_spelling(x, variables) for x in value.parameters], ", ") * "}"
    elseif value isa Union{Bool,Integer,Char,Symbol,AbstractFloat}
        return repr(value)
    elseif value isa Tuple
        values = [type_spelling(part, variables) for part in value]
        return "(" * join(values, ", ") * (length(values) == 1 ? "," : "") * ")"
    end
    error("Unsupported Julia dispatch parameter of type $(typeof(value)).")
end

function type_variables(value, outer=IdDict{TypeVar,String}())
    scope = copy(outer)
    constraints = String[]
    while value isa UnionAll
        var = value.var
        name = "T$(length(scope) + 1)"
        # A bound can refer to an enclosing variable.
        lower = var.lb === Union{} ? "" : type_spelling(var.lb, scope) * "<:"
        upper = var.ub === Any ? "" : "<:" * type_spelling(var.ub, scope)
        push!(constraints, lower * name * upper)
        scope[var] = name
        value = value.body
    end
    return value, constraints, scope
end

where_clause(constraints) = isempty(constraints) ? "" : " where {" * join(constraints, ", ") * "}"

function package_file(root, path)
    isabspath(path) || return nothing
    local_path = relpath(normpath(path), root)
    parts = splitpath(local_path)
    return isabspath(local_path) || any(==(".."), parts) ? nothing : join(parts, '/')
end

function source_receipt(root, path, mod; cached=nothing)
    relative = package_file(root, path)
    relative === nothing && error("A Julia package include escapes the configured checkout.")
    canonical = package_file(root, realpath(path))
    canonical === nothing && error("A Julia package include symlink escapes the configured checkout.")
    bytes = read(path)
    cached === nothing || sha(bytes) == sha(cached) ||
        error("Julia source changed since the loaded package was precompiled: $relative")
    return Dict("path" => relative, "canonical" => canonical, "module" => mod,
                "digest" => sha(bytes))
end

function verify_inputs(root, receipts)
    for input in receipts
        path = joinpath(root, input["path"])
        (package_file(root, realpath(path)) == input["canonical"] &&
         sha(read(path)) == input["digest"]) || error("Julia inputs changed while loading the checkout.")
    end
end

function cached_inputs!(receipts, root, identity)
    cached = NativeCompat.cache_path(identity)
    cached === nothing && return true
    includes, sources = NativeCompat.cache_files(cached)
    source_names = Set(source.filename for source in sources)
    complete = true
    for input in includes
        if input.filename ∉ source_names
            # Files registered via include_dependency may have a timestamp only
            # on older Julia releases. The compiler still checks them on load.
            complete = false
            continue
        end
        text = NativeCompat.cache_source(cached, input.filename)
        mod = join([identity.name; input.modpath], '.')
        push!(receipts, source_receipt(root, input.filename, mod; cached=text))
    end
    return complete
end

function location(root, path, line)
    path isa Symbol && (path = String(path))
    path isa AbstractString || return Dict{String,Any}()
    relative = package_file(root, path)
    relative === nothing && return Dict{String,Any}()
    result = Dict{String,Any}("path" => relative)
    line isa Integer && line > 0 && (result["line"] = Int(line))
    return result
end

function documentation_text(doc)::Dict{String,Any}
    if !isempty(doc.text) && all(part -> part isa Union{String,SubString{String},Bool,Number,Char,Symbol},
                                 doc.text)
        return Dict("text" => join(doc.text))
    elseif doc.object isa Markdown.MD
        return Dict("text" => sprint(show, MIME"text/markdown"(), doc.object))
    end
    return Dict("unsupported" => "Unsupported Julia documentation object.")
end

function document(root, doc, registered)
    value = documentation_text(doc)
    value["module"] = module_name(registered)
    merge!(value, location(root, get(doc.data, :path, nothing), get(doc.data, :linenumber, 0)))
    return value
end

function methods_for(value, modules)
    entries = value isa Type ? NativeCompat.constructor_methods(value) : collect(methods(value, modules))
    return unique(filter(method -> method.module in modules, entries))
end

function positional(method, macro_binding)
    sig = Base.unwrap_unionall(method.sig)
    first = macro_binding ? 4 : 2
    return [sig.parameters[index] for index in first:length(sig.parameters)]
end

function method_arguments(method, macro_binding)
    removed = macro_binding ? 3 : 1
    arguments = NativeCompat.arguments(method)
    return [Base.isidentifier(name) && !startswith(String(name), '#') ? String(name) : "arg$index"
            for (index, name) in enumerate(arguments[removed+1:end])]
end

# Resolve type syntax without evaluating package expressions a second time.
# Unsupported expressions leave the reflected signature in charge.
function dispatch_value(expr, mod, scope)
    if expr isa Symbol
        return haskey(scope, expr) ? scope[expr] : getfield(mod, expr)
    elseif expr isa QuoteNode && expr.value isa Symbol
        return expr.value
    elseif expr isa Union{Bool,Integer,Char,AbstractFloat}
        return expr
    elseif Meta.isexpr(expr, :., 2)
        owner = dispatch_value(expr.args[1], mod, scope)
        name = expr.args[2]
        owner isa Module && name isa QuoteNode && return getfield(owner, name.value)
    elseif Meta.isexpr(expr, :curly)
        head = dispatch_value(first(expr.args), mod, scope)
        anonymous = TypeVar[]
        params = map(expr.args[2:end]) do arg
            if Meta.isexpr(arg, (:<:, :>:), 1)
                bound = dispatch_value(only(arg.args), mod, scope)
                variable = arg.head === :<: ? TypeVar(gensym(), Union{}, bound) : TypeVar(gensym(), bound, Any)
                push!(anonymous, variable)
                return variable
            end
            return dispatch_value(arg, mod, scope)
        end
        return wrap_dispatch(Core.apply_type(head, params...), anonymous)
    elseif Meta.isexpr(expr, :where)
        nested = copy(scope)
        variables = dispatch_variables(expr.args[2:end], mod, nested)
        return wrap_dispatch(dispatch_value(first(expr.args), mod, nested), variables)
    elseif Meta.isexpr(expr, :tuple)
        return Tuple(dispatch_value(arg, mod, scope) for arg in expr.args)
    elseif Meta.isexpr(expr, :call, 2) && expr.args[1] === :typeof && getfield(mod, :typeof) === typeof
        return typeof(dispatch_value(expr.args[2], mod, scope))
    end
    throw(ArgumentError("Unsupported source type expression."))
end

function dispatch_variables(specs, mod, scope)
    return map(specs) do spec
        name, lower, upper = if spec isa Symbol
            (spec, Union{}, Any)
        elseif Meta.isexpr(spec, :<:, 2)
            (spec.args[1], Union{}, dispatch_value(spec.args[2], mod, scope))
        elseif Meta.isexpr(spec, :>:, 2)
            (spec.args[1], dispatch_value(spec.args[2], mod, scope), Any)
        elseif Meta.isexpr(spec, :comparison, 5) && spec.args[2] === spec.args[4] === :<:
            (spec.args[3], dispatch_value(spec.args[1], mod, scope),
             dispatch_value(spec.args[5], mod, scope))
        else
            throw(ArgumentError("Unsupported source type variable."))
        end
        variable = TypeVar(name, lower, upper)
        scope[name] = variable
        return variable
    end
end

wrap_dispatch(type, variables) = foldr(UnionAll, variables; init=type)

function dispatch_argument(arg, mod, scope)
    Meta.isexpr(arg, :kw) && return dispatch_argument(first(arg.args), mod, scope)
    if Meta.isexpr(arg, :(...), 1)
        return Vararg{dispatch_argument(only(arg.args), mod, scope)}
    end
    Meta.isexpr(arg, :(::)) && return dispatch_value(last(arg.args), mod, scope)
    (arg isa Symbol || Meta.isexpr(arg, :tuple)) && return Any
    throw(ArgumentError("Unsupported source argument."))
end

function source_dispatch(expr, mod, value, macro_binding)
    scope = Dict{Symbol,TypeVar}()
    variables = TypeVar[]
    try
        while expr isa Expr
            if expr.head === :where
                append!(variables, dispatch_variables(expr.args[2:end], mod, scope))
            elseif expr.head === :(::) && Meta.isexpr(last(expr.args), :where)
                # A return annotation can carry the method's where clause.
                append!(variables, dispatch_variables(last(expr.args).args[2:end], mod, scope))
            elseif expr.head === :macrocall
                NativeCompat.macro_name(first(expr.args)) in
                    (Symbol("@inline"), Symbol("@noinline"), Symbol("@propagate_inbounds")) || return nothing
                expr = last(expr.args)
                continue
            elseif expr.head === :call
                callee = first(expr.args)
                macro_binding && callee isa Symbol && (callee = Symbol('@', callee))
                target = dispatch_value(callee, mod, scope)
                head = if value isa Type
                    base = Base.unwrap_unionall(target)
                    base isa DataType && base.name === Base.unwrap_unionall(value).name ||
                        return nothing
                    Type{target}
                else
                    target === value || return nothing
                    typeof(value)
                end
                types = [dispatch_argument(arg, mod, scope) for arg in expr.args[2:end]
                         if !Meta.isexpr(arg, :parameters)]
                return wrap_dispatch(Tuple{head, types...}, variables)
            elseif expr.head ∉ (:function, :(=), :macro, :(::))
                return nothing
            end
            expr = first(expr.args)
        end
    catch err
        err isa Union{ArgumentError,TypeError,MethodError,UndefVarError} || rethrow()
    end
    return nothing
end

function live_dispatch(method, macro_binding)
    macro_binding || return method.sig
    head = Base.unwrap_unionall(method.sig).parameters[1]
    return Base.rewrap_unionall(Tuple{head, positional(method, true)...}, method.sig)
end

function method_record(root, method, value, name, macro_binding, headers)
    _, constraints, variables = type_variables(method.sig)
    arguments = method_arguments(method, macro_binding)
    positionals = positional(method, macro_binding)
    types = [type_spelling(type, variables) for type in positionals]
    live = live_dispatch(method, macro_binding)
    offsets = [offset for (offset, header) in headers
               if source_dispatch(header, method.module, value, macro_binding) == live]
    keywords = [String(key) for key in NativeCompat.keywords(method) if key != Symbol("...")]
    name = macro_binding ? name[2:end] : name
    binding = Base.isoperator(Symbol(name)) ? name : name_token(Symbol(name))
    if value isa Type
        head = NativeCompat.constructor_head(method, value)
        if head !== value
            parameters = Base.unwrap_unionall(head).parameters
            binding *= "{" * join([type_spelling(type, variables) for type in parameters], ", ") * "}"
        end
    end
    signatures = [type == "Any" ? argument : argument * "::" * type
                  for (argument, type) in zip(arguments, types)]
    args = join(signatures, ", ")
    isempty(keywords) || (args *= "; " * join(keywords, ", "))
    display = binding * "(" * args * ")" * where_clause(constraints)
    selector = binding * "(" * join(["::" * type for type in types], ", ") * ")" * where_clause(constraints)
    return merge(Dict{String,Any}(
        "module" => module_name(method.module), "types" => types, "arguments" => arguments,
        "source_offsets" => offsets,
        "keywords" => keywords, "display" => display, "selector" => selector, "docs" => Any[],
    ), location(root, method.file, method.line))
end

function attached_method(entries, signature, macro_binding, doc, value, registered, headers)
    docs_body = Base.unwrap_unionall(signature)
    branches = Base.uniontypes(docs_body)
    isempty(branches) && return nothing
    # Docs uses singleton type-variable tuples to preserve bindings, but one
    # can also be the full method after Docs adds the optional-arity branches.
    # Match the live methods first and prefer the widest matching arity.
    marker(branch) = length(branch.parameters) == 1 && branch.parameters[1] isa TypeVar
    required = maximum((length(branch.parameters) for branch in branches if !marker(branch)); init=0)
    keys = [Base.rewrap_unionall(branch, signature) => !marker(branch) for branch in branches
            if length(branch.parameters) >= required]
    priorities = map(entries) do method
        # Rewrapping preserves bound variables; recreating them from display
        # strings would conflate unrelated types with the same display name.
        types = positional(method, macro_binding)
        call = Base.rewrap_unionall(Tuple{types...}, method.sig)
        # At the same arity a concrete branch is stronger evidence than a
        # marker, which could also match an unrelated overload.
        maximum(((length(types), concrete) for (key, concrete) in keys if key == call);
                init=(-1, false))
    end
    matches = findall(priority -> first(priority) >= 0, priorities)
    if length(matches) > 1
        authored = [source_dispatch(header, registered, value, macro_binding) for header in headers]
        selected = filter(index -> any(==(live_dispatch(entries[index], macro_binding)), authored), matches)
        length(selected) == 1 && return only(selected)
        best = maximum(index -> priorities[index], matches)
        filter!(index -> priorities[index] == best, matches)
        path = get(doc.data, :path, nothing)
        line = get(doc.data, :linenumber, 0)
        local_matches = filter(index -> String(entries[index].file) == path &&
                                        entries[index].line >= line, matches)
        if !isempty(local_matches)
            nearest = minimum(index -> entries[index].line, local_matches)
            matches = filter(index -> entries[index].line == nearest, local_matches)
        end
    end
    isempty(matches) && return nothing
    length(matches) == 1 || error("a documentation attachment matches multiple live methods")
    return only(matches)
end

function type_record(value)
    body, parameters, variables = type_variables(value)
    flavor = isabstracttype(body) ? "abstract" : isprimitivetype(body) ? "primitive" : "struct"
    fields = [Dict("name" => String(name), "type" => type_spelling(fieldtype(body, index), variables),
                   "const" => ismutabletype(body) && Base.isconst(body, index), "docs" => Any[])
              for (index, name) in enumerate(flavor == "abstract" ? () : fieldnames(body))]
    return Dict{String,Any}("flavor" => flavor, "mutable" => ismutabletype(body),
                            "bits" => flavor == "primitive" ? 8 * sizeof(body) : 0,
                            "supertype" => type_spelling(supertype(body), variables),
                            "parameters" => parameters, "fields" => fields)
end

function canonical_binding(mod, name, value)
    if value isa Module
        return parentmodule(value), nameof(value)
    end
    if value isa Function || Base.unwrap_unionall(value) isa DataType
        owner = parentmodule(value)
        symbol = nameof(value)
        if isdefined(owner, symbol) && getfield(owner, symbol) === value
            return owner, symbol
        end
    end
    origin = NativeCompat.binding_origin(mod, name)
    return origin.mod, origin.name
end

function binding_name(mod, name, value)
    value isa Module && return module_name(value)
    owner, symbol = canonical_binding(mod, name, value)
    return qualified(owner, symbol)
end

function package_modules(root)
    modules = Module[root]
    for mod in modules
        for name in names(mod; all=true, imported=true)
            isdefined(mod, name) || continue
            value = getfield(mod, name)
            if value isa Module && parentmodule(value) === mod && value ∉ modules
                push!(modules, value)
            end
        end
    end
    return modules
end

function collect_surface(root, loaded, receipts)
    modules = package_modules(loaded)
    bindings = Dict{String,Tuple{Module,Symbol,Any}}()
    aliases = Dict{String,Vector{Dict{String,String}}}()
    attachments = Dict{String,Vector{Tuple{Module,Any,Any}}}()
    public = Dict{String,String}()
    exports = Dict{String,String}[]
    errors = String[]
    for mod in sort!(collect(modules); by=module_name)
        bindings[module_name(mod)] = (parentmodule(mod), nameof(mod), mod)
        for name in sort!(union(names(mod; all=true, imported=true), names(mod)); by=string)
            name === nameof(mod) && continue
            startswith(String(name), '#') && continue
            visible = NativeCompat.public_binding(mod, name)
            if !isdefined(mod, name)
                visible && push!(errors, "Public Julia binding `$(qualified(mod, name))` is undefined.")
                continue
            end
            value = getfield(mod, name)
            canonical = binding_name(mod, name, value)
            local_name = qualified(mod, name)
            owner, symbol = canonical_binding(mod, name, value)
            bindings[canonical] = (owner, symbol, value)
            if canonical != local_name
                push!(get!(aliases, canonical, []),
                      Dict("name" => local_name, "kind" => Base.binding_module(mod, name) === mod ?
                           "assignment" : "import"))
            end
            if visible
                visibility = Base.isexported(mod, name) ? "exported" : "public"
                push!(exports, Dict("module" => module_name(mod), "name" => String(name),
                                    "visibility" => visibility))
                if get(public, canonical, "private") != "exported"
                    public[canonical] = visibility
                end
            end
        end
        table = NativeCompat.documentation(mod)
        table === nothing && continue
        for (binding, docs) in table
            if !Base.Docs.defined(binding)
                push!(errors, "Julia documentation targets an undefined binding `$(qualified(binding.mod, binding.var))`.")
                continue
            end
            value = Base.Docs.resolve(binding)
            canonical = binding_name(binding.mod, binding.var, value)
            owner, symbol = canonical_binding(binding.mod, binding.var, value)
            bindings[canonical] = (owner, symbol, value)
            for signature in docs.order
                push!(get!(attachments, canonical, []), (mod, signature, docs.docs[signature]))
            end
        end
    end

    retained = union(keys(public), keys(attachments), [module_name(loaded)])
    for name in collect(retained)
        haskey(bindings, name) || continue
        mod, _, _ = bindings[name]
        while mod in modules
            push!(retained, module_name(mod))
            mod === loaded && break
            mod = parentmodule(mod)
        end
    end

    for mod in modules
        explicit = Set(names(mod; all=true, imported=true))
        candidates = Set(name for used in NativeCompat.used_modules(mod) for name in names(used)
                         if Base.isexported(used, name))
        for name in setdiff(candidates, explicit)
            # Ask Julia to resolve the name so that shadows and ambiguous
            # exports cannot introduce a lookup alias.
            isdefined(mod, name) && Base.binding_module(mod, name) !== mod || continue
            canonical = binding_name(mod, name, getfield(mod, name))
            canonical in retained || continue
            local_name = qualified(mod, name)
            canonical == local_name && continue
            entries = get!(aliases, canonical, [])
            any(alias -> alias["name"] == local_name, entries) ||
                push!(entries, Dict("name" => local_name, "kind" => "import"))
        end
    end

    tracked = Set(joinpath(root, input[key]) for input in receipts for key in ("path", "canonical"))
    sources = Dict{String,NamedTuple}()
    function headers(path)
        path isa Union{String,Symbol} || return nothing
        file = normpath(String(path))
        file in tracked || return nothing
        return get!(() -> NativeCompat.source_headers(file), sources, file)
    end

    items = Any[]
    for canonical in sort!(collect(retained))
        owner, symbol, value = bindings[canonical]
        name = String(symbol)
        observed = Dict{String,Any}(
            "name" => name, "qualified_name" => canonical,
            "module" => value === loaded ? "" : owner in modules ? module_name(owner) : "",
            "aliases" => sort!(get(aliases, canonical, []); by=alias -> alias["name"]),
            "docs" => Any[], "methods" => Any[],
        )
        docs = get(attachments, canonical, [])
        try
            macro_binding = value isa Function && startswith(name, '@')
            entries = Method[]
            if value isa Module && value in modules
                observed["kind"] = "module"
            elseif value isa Union{DataType,UnionAll} && owner in modules && nameof(value) === symbol
                observed["kind"] = "type"
                observed["type"] = type_record(value)
                entries = methods_for(value, modules)
            elseif value isa Function && !(value isa Core.Builtin)
                observed["kind"] = macro_binding ? "macro" : "function"
                entries = methods_for(value, modules)
                if owner ∉ modules
                    isempty(entries) && error("external-only reexports have no package-owned methods")
                    observed["module"] = module_name(first(sort!(entries; by=m -> module_name(m.module))).module)
                end
            elseif owner in modules && isconst(owner, symbol)
                observed["kind"] = "constant"
                observed["annotation"] = type_spelling(typeof(value), IdDict{TypeVar,String}())
            else
                error("the binding's runtime declaration kind is unsupported")
            end
            for entry in entries
                source = headers(entry.file)
                declarations = source === nothing ? () : get(source.declarations, entry.line, ())
                push!(observed["methods"], method_record(root, entry, value, name, macro_binding, declarations))
            end
            for (registered, signature, doc) in docs
                parsed = document(root, doc, registered)
                if signature === Union{} || macro_binding
                    push!(observed["docs"], parsed)
                    if observed["kind"] == "type"
                        for field in observed["type"]["fields"]
                            field_doc = get(get(doc.data, :fields, Dict()), Symbol(field["name"]), nothing)
                            if field_doc isa AbstractString
                                push!(field["docs"], merge(copy(parsed),
                                                           Dict("text" => String(field_doc))))
                                delete!(only(field["docs"]), "unsupported")
                            end
                        end
                    end
                else
                    source = headers(get(doc.data, :path, nothing))
                    authored = source === nothing ? () : get(source.attachments, get(doc.data, :linenumber, 0), ())
                    index = attached_method(entries, signature, macro_binding, doc, value, registered, authored)
                    index === nothing && error("a documentation attachment has no exact live method")
                    push!(observed["methods"][index]["docs"], parsed)
                end
            end
            sort!(observed["methods"]; by=method -> (get(method, "path", ""),
                   get(method, "line", 0), method["selector"], method["module"]))
            push!(items, observed)
        catch err
            push!(errors, "Julia binding `$canonical`: $(sprint(showerror, err))")
        end
    end
    return items, exports, sort!(unique(errors))
end

# Julia resolves dependencies in the prepared environment. Verify both the
# checkout identity and the source actually used by the loader.
function observe_checkout(package_path, entry_path, output_path)
    VERSION.major == 1 && 10 <= VERSION.minor <= 13 ||
        error("Julia native extraction supports Julia 1.10 through 1.13.")
    timings = splitext(output_path)[1] * ".timings"
    started = time_ns()
    root = realpath(package_path)
    project_path = joinpath(root, "Project.toml")
    project_bytes = read(project_path)
    project = TOML.parse(String(copy(project_bytes)))
    identity = Base.PkgId(Base.UUID(project["uuid"]), project["name"])
    receipts = Dict{String,String}[]
    entry = realpath(entry_path)
    callback = (mod, file) -> begin
        if Base.PkgId(mod).uuid == identity.uuid || file == entry
            scope = file == entry ? "" : module_name(mod)
            push!(receipts, source_receipt(root, file, scope))
        end
        nothing
    end
    push!(Base.include_callbacks, callback)
    loaded = try
        Base.require(identity)
    finally
        filter!(value -> value !== callback, Base.include_callbacks)
    end
    Base.PkgId(loaded) == identity || error("Loaded package identity conflicts with Project.toml.")
    nameof(loaded) == Symbol(project["name"]) || error("Loaded package name conflicts with Project.toml.")
    version = Base.pkgversion(loaded)
    if haskey(project, "version")
        version == VersionNumber(project["version"]) || error("Loaded package version conflicts with Project.toml.")
    end
    loaded_entry = pathof(loaded)
    loaded_entry !== nothing && realpath(loaded_entry) == entry ||
        error("Loaded package does not match the supplied checkout.")
    complete = cached_inputs!(receipts, root, identity)
    for input in receipts
        input["canonical"] == package_file(root, entry) && (input["module"] = "")
    end
    any(input -> input["canonical"] == package_file(root, entry), receipts) ||
        error("Julia did not report the loaded checkout entry.")
    read(project_path) == project_bytes || error("Julia project changed while loading the checkout.")
    verify_inputs(root, receipts)

    write(timings, "package-loading\t$(Float64(time_ns() - started) / 1e9)\n")
    started = time_ns()
    items, exports, errors = Base.invokelatest(collect_surface, root, loaded, receipts)
    dependencies = Dict(string(id.uuid) => string(version) for (id, dependency) in Base.loaded_modules
                        if (version = Base.pkgversion(dependency)) !== nothing)
    result = Dict(
        "policy" => EXPORT_POLICY, "julia_version" => string(VERSION),
        "package_name" => identity.name, "package_uuid" => string(identity.uuid),
        "package_version" => string(something(Base.pkgversion(loaded), "")),
        "project_digest" => sha(project_bytes), "dependencies" => dependencies,
        "inputs" => sort!(unique(receipts); by=input -> (input["path"], input["module"])),
        "complete" => complete, "items" => items, "exports" => exports, "errors" => errors,
    )
    verify_inputs(root, receipts)
    open(timings, "a") do io
        println(io, "extraction\t$(Float64(time_ns() - started) / 1e9)")
    end
    open(output_path, "w") do io
        TOML.print(io, result; sorted=true)
    end
end

if abspath(PROGRAM_FILE) == @__FILE__
    observe_checkout(ARGS...)
end
