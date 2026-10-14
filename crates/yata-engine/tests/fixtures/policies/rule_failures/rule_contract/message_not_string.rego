package yata.rules.test_policies.rule_contract.message_not_string

import rego.v1

metadata := {
	"title": "Rule contract violation: message_not_string",
	"severity": "medium",
	"description": "Gives a number for message.",
	"remediation": "Nothing to fix.",
	"target_type": "aws.example.widget",
	"references": [],
}

default fail := false

fail if input.tags.flag == "bad"

message := 5
