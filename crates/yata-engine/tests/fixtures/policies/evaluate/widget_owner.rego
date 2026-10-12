package yata.rules.eval_policies.widget_owner

import rego.v1

metadata := {
	"title": "Widget must have an owner tag",
	"severity": "high",
	"description": "Widgets need an owner tag.",
	"remediation": "Add an owner tag.",
	"target_type": "aws.example.widget",
	"related_types": ["aws.example.gadget"],
	"references": [],
}

fail contains finding if {
	some widget in input.aws.example.widget
	not widget.tags.owner
	finding := {
		"resource_id": widget.id,
		"message": sprintf("Widget %s has no owner tag", [widget.id]),
		"path": "/tags",
		"actual": widget.tags,
		"expected": {"owner": "any"},
	}
}

# A widget named "nullish" reports explicit null values (null is not the same as absent).
fail contains finding if {
	some widget in input.aws.example.widget
	widget.name == "nullish"
	finding := {
		"resource_id": widget.id,
		"message": "null values",
		"actual": null,
		"expected": null,
	}
}
