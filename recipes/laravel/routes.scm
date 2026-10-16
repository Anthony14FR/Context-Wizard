(scoped_call_expression
  scope: (name) @_scope (#eq? @_scope "Route")
  name: (name) @verb (#match? @verb "^(get|post|put|patch|delete|options|any|match|resource|apiResource|view|redirect)$")
  arguments: (arguments
    .
    (argument (_) @uri)
    .
    (argument (_) @action)))
