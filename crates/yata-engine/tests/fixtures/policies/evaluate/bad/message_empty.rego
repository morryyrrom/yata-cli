package yata.rules.eval_policies.bad.message_empty

import rego.v1

metadata := {
	"title": "Rule contract violation: message_empty",
	"severity": "low",
	"description": "Gives an empty string for message when the widget is named bad.",
	"remediation": "None.",
	"target_type": "aws.example.widget",
	"references": [],
}

default fail := false

fail if input.name == "bad"

message := ""
