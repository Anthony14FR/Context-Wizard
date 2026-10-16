(member_call_expression
  object: (variable_name) @_table (#eq? @_table "$table")
  name: (name) @type (#not-match? @type "^(index|unique|primary|foreign|dropColumn|dropIndex|dropForeign|renameColumn|timestamps|softDeletes|rememberToken)$")
  arguments: (arguments . (argument (_) @column)))
