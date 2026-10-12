package yata.rules.eval_policies.bad.not_object

import rego.v1

metadata := {
	"title": "Malformed fail element: not_object",
	"severity": "low",
	"description": "Returns a malformed or unmappable fail element.",
	"remediation": "None.",
	"target_type": "aws.example.widget",
	"references": [],
}

fail contains "just a string"
