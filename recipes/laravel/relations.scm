(method_declaration
  name: (name) @relation
  body: (compound_statement
    (return_statement
      (member_call_expression
        object: (variable_name) @_this (#eq? @_this "$this")
        name: (name) @kind (#match? @kind "^(hasOne|hasMany|belongsTo|belongsToMany|hasOneThrough|hasManyThrough|morphTo|morphOne|morphMany|morphToMany|morphedByMany)$")
        arguments: (arguments . (argument (_) @target)?)))))

(method_declaration
  name: (name) @relation
  body: (compound_statement
    (return_statement
      (member_call_expression
        object: (member_call_expression
          object: (variable_name) @_this (#eq? @_this "$this")
          name: (name) @kind (#match? @kind "^(hasOne|hasMany|belongsTo|belongsToMany|hasOneThrough|hasManyThrough|morphTo|morphOne|morphMany|morphToMany|morphedByMany)$")
          arguments: (arguments . (argument (_) @target)?))))))
