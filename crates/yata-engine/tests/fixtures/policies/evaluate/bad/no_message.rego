package yata.rules.eval_policies.bad.no_message

import rego.v1

metadata := {
	"title": "Malformed fail element: no_message",
	"severity": "low",
	"description": "Returns a malformed or unmappable fail element.",
	"remediation": "None.",
	"target_type": "aws.example.widget",
	"references": [],
}

fail contains {"resource_id": "w-1"}
