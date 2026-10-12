package yata.rules.eval_policies.bad.region_not_string

import rego.v1

metadata := {
	"title": "Malformed fail element: region_not_string",
	"severity": "low",
	"description": "Returns a malformed or unmappable fail element.",
	"remediation": "None.",
	"target_type": "aws.example.widget",
	"references": [],
}

fail contains {"resource_id": "w-1", "resource_region": 5, "message": "m"}
