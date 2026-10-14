package yata.rules.eval_policies.bad.message_conflict

import rego.v1

metadata := {
	"title": "Rule contract violation: message_conflict",
	"severity": "low",
	"description": "Gives two messages when the widget is named bad.",
	"remediation": "None.",
	"target_type": "aws.example.widget",
	"references": [],
}

default fail := false

fail if input.name == "bad"

message := "one" if input.id

message := "two" if input.id
