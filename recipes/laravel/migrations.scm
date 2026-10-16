(scoped_call_expression
  scope: (name) @_scope (#eq? @_scope "Schema")
  name: (name) @operation (#match? @operation "^(create|table|drop|dropIfExists|rename)$")
  arguments: (arguments . (argument (_) @table)))
