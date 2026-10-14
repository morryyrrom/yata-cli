package yata.rules.cli_unsupported_policies.unsupported_widget

import rego.v1

metadata := {
	"title": "Unsupported widget must have an owner",
	"severity": "low",
	"description": "Uses a resource type that no provider supports.",
	"remediation": "None.",
	"target_type": "aws.example.unsupported",
	"references": [],
}

fail contains finding if {
	some w in input.aws.example.unsupported
	not w.tags.owner
	finding := {"resource_id": w.id, "message": sprintf("Widget %s has no owner", [w.id])}
}
