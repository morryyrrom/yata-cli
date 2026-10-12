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

fail contains finding if {
	some widget in input.aws.example.widget
	not widget.tags.owner
	not helpers.is_error_marker(object.get(widget.configuration, "Describe", null))
	finding := {
		"resource_id": widget.id,
		"message": sprintf("Widget %s has no owner tag", [widget.id]),
		"path": "/tags",
		"actual": widget.tags,
	}
}
