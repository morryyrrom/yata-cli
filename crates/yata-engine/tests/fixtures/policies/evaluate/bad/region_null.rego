package yata.rules.eval_policies.bad.region_null

import rego.v1

metadata := {
	"title": "Malformed fail element: region_null",
	"severity": "low",
	"description": "Returns a malformed or unmappable fail element.",
	"remediation": "None.",
	"target_type": "aws.example.widget",
	"references": [],
}

fail contains {"resource_id": "w-1", "resource_region": null, "message": "m"}
