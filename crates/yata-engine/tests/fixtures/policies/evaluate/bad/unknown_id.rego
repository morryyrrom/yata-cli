package yata.rules.eval_policies.bad.unknown_id

import rego.v1

metadata := {
	"title": "Malformed fail element: unknown_id",
	"severity": "low",
	"description": "Returns a malformed or unmappable fail element.",
	"remediation": "None.",
	"target_type": "aws.example.widget",
	"references": [],
}

fail contains {"resource_id": "no-such-widget", "message": "m"}
