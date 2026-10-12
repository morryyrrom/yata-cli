package yata.rules.eval_policies.bad.unknown_id_region

import rego.v1

metadata := {
	"title": "Malformed fail element: unknown_id_region",
	"severity": "low",
	"description": "Returns a malformed or unmappable fail element.",
	"remediation": "None.",
	"target_type": "aws.example.widget",
	"references": [],
}

fail contains {"resource_id": "w-1", "resource_region": "eu-west-3", "message": "m"}
