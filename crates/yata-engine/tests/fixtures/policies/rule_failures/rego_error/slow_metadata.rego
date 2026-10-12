package yata.rules.test_policies.rego_error.slow_metadata

import rego.v1

# The metadata does not depend on input, but takes far longer than any test time limit.
metadata := {
	"title": "Slow metadata",
	"severity": "low",
	"description": "d",
	"remediation": "r",
	"target_type": "aws.example.widget",
	"references": [],
	"tags": [sprintf("%d", [count(slow)])],
}

slow := {1 |
	some i in numbers.range(1, 3000)
	some j in numbers.range(1, 3000)
	some k in numbers.range(1, 3000)
	i + j + k < 0
}
