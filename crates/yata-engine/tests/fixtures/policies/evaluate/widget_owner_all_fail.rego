package yata.rules.eval_policies.widget_owner_all_fail

import rego.v1

metadata := {
	"title": "Fails every flagged widget",
	"severity": "low",
	"description": "Fails the widgets with the tag flag=bad, with a message and a reason.",
	"remediation": "None.",
	"target_type": "aws.example.widget",
	"references": [],
}

default fail := false

fail if input.tags.flag == "bad"

message := sprintf("Widget %s is flagged", [input.id])

reason := {
	"path": "/tags/flag",
	"actual": input.tags.flag,
	"expected": "good",
}
