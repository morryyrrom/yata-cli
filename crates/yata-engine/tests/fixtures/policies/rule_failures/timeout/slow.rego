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

# `input.id` needs a widget, so loading (no input) finishes at once. With a widget the nested
# loops would run for far longer than any test time limit.
default fail := false

fail if {
	input.id
	some i in numbers.range(1, 5000)
	some j in numbers.range(1, 5000)
	some k in numbers.range(1, 5000)
	i + j + k < 0
}
