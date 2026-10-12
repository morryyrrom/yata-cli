package yata.rules.eval_policies.bad.no_resource_id

import rego.v1

metadata := {
	"title": "Malformed fail element: no_resource_id",
	"severity": "low",
	"description": "Returns a malformed or unmappable fail element.",
	"remediation": "None.",
	"target_type": "aws.example.widget",
	"references": [],
}

fail contains {"message": "m"}
