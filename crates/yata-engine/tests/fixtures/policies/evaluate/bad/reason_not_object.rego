package yata.rules.eval_policies.bad.reason_not_object

import rego.v1

metadata := {
	"title": "Rule contract violation: reason_not_object",
	"severity": "low",
	"description": "Gives an array for reason when the widget is named bad.",
	"remediation": "None.",
	"target_type": "aws.example.widget",
	"references": [],
}

default fail := false

fail if input.name == "bad"

reason := ["a"]
