package yata.rules.test_policies.timeout.slow

import rego.v1

metadata := {
	"title": "Slow when there is a widget",
	"severity": "low",
	"description": "Loops over statements for every widget; it ends at once without input.",
	"remediation": "None.",
	"target_type": "aws.example.widget",
	"references": [],
}

# The first statement needs a widget, so loading (no input) finishes at once. With a widget the
# nested loops would run for far longer than any test time limit.
fail contains finding if {
	some widget in input.aws.example.widget
	some i in numbers.range(1, 5000)
	some j in numbers.range(1, 5000)
	some k in numbers.range(1, 5000)
	i + j + k < 0
	finding := {"resource_id": widget.id, "message": "never"}
}
