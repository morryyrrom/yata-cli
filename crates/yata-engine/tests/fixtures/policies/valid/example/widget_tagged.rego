package yata.rules.test_policies.example.widget_tagged

import data.yata.lib.test_policies.helpers
import rego.v1

metadata := {
	"title": "Widget must have an owner tag",
	"severity": "high",
	"description": "Widgets need an owner tag.",
	"remediation": "Add an owner tag.",
	"target_type": "aws.example.widget",
	"related_types": ["aws.example.gadget"],
	"references": [{"standard": "example-standard", "version": "1.0.0", "control": "1.1"}],
	"tags": ["example"],
}

default fail := false

fail if {
	not input.tags.owner
	not helpers.is_error_marker(object.get(input.configuration, "Describe", null))
}

message := sprintf("Widget %s has no owner tag", [input.id])

reason := {
	"path": "/tags",
	"actual": input.tags,
	"expected": {"owner": "any"},
}
