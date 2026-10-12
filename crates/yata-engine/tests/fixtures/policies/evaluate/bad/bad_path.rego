package yata.rules.eval_policies.bad.bad_path

import rego.v1

metadata := {
	"title": "Malformed fail element: bad_path",
	"severity": "low",
	"description": "Returns a malformed or unmappable fail element.",
	"remediation": "None.",
	"target_type": "aws.example.widget",
	"references": [],
}

fail contains {"resource_id": "w-1", "message": "m", "path": "tags/owner"}
