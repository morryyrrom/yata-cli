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

default fail := false

fail if input.name == "nullish"

fail if not input.tags.owner

# A widget named "nullish" reports explicit null values (null is not the same as absent).
message := "null values" if {
	input.name == "nullish"
} else := sprintf("Widget %s has no owner tag", [input.id])

reason := {"actual": null, "expected": null} if {
	input.name == "nullish"
} else := {"path": "/tags", "actual": input.tags, "expected": {"owner": "any"}}
