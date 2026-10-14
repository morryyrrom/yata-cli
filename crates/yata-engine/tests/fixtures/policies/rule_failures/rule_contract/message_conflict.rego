package yata.rules.test_policies.rule_contract.message_conflict

import rego.v1

metadata := {
	"title": "Rule contract violation: message_conflict",
	"severity": "medium",
	"description": "Gives two messages for a widget flagged bad.",
	"remediation": "Nothing to fix.",
	"target_type": "aws.example.widget",
	"references": [],
}

default fail := false

fail if input.tags.flag == "bad"

message := "one" if input.id

message := "two" if input.id
