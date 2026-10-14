package yata.rules.eval_policies.bad.message_not_string

import rego.v1

metadata := {
	"title": "Rule contract violation: message_not_string",
	"severity": "low",
	"description": "Gives a number for message when the widget is named bad.",
	"remediation": "None.",
	"target_type": "aws.example.widget",
	"references": [],
}

default fail := false

fail if input.name == "bad"

message := 5
