package yata.rules.eval_policies.slow_total

import rego.v1

metadata := {
	"title": "Slow in total, fast for each widget",
	"severity": "low",
	"description": "Each widget takes a time set by its Spin configuration value.",
	"remediation": "None.",
	"target_type": "aws.example.widget",
	"references": [],
}

# The time for one widget grows with `Spin`; many widgets add up. Without input it ends at once.
default fail := false

fail if {
	spin := input.configuration.Spin
	some i in numbers.range(1, spin)
	some j in numbers.range(1, 100)
	i + j < 0
}
