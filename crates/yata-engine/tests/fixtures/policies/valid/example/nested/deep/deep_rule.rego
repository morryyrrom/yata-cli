package yata.rules.test_policies.example.nested.deep.deep_rule

import rego.v1

metadata := {
	"title": "Deeply nested rule",
	"severity": "info",
	"description": "Checks that rules can be nested in several directories.",
	"remediation": "Nothing to fix.",
	"target_type": "aws.example.widget",
	"references": [],
}

default fail := false

fail if input.tags.flag == "bad"

message := sprintf("Widget %s is flagged", [input.id])
