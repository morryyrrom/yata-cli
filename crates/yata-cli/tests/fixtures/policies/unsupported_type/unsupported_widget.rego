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

default fail := false

fail if not input.tags.owner

message := sprintf("Widget %s has no owner", [input.id])
