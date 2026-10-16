(function_declaration
  name: (identifier) @hook (#match? @hook "^use[A-Z]")
  parameters: (formal_parameters) @parameters)

(variable_declarator
  name: (identifier) @hook (#match? @hook "^use[A-Z]")
  value: (arrow_function
    parameters: (formal_parameters) @parameters))
