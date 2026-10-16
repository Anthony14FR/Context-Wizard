(function_declaration
  name: (identifier) @component (#match? @component "^[A-Z]")
  parameters: (formal_parameters) @props)

(variable_declarator
  name: (identifier) @component (#match? @component "^[A-Z]")
  value: (arrow_function
    parameters: (formal_parameters) @props))

(variable_declarator
  name: (identifier) @component (#match? @component "^[A-Z]")
  value: (call_expression
    arguments: (arguments
      (arrow_function
        parameters: (formal_parameters) @props))))
