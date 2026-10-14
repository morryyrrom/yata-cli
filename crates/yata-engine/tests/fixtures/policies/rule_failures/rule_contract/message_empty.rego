package yata.rules.test_policies.rule_contract.message_empty

import rego.v1

metadata := {
	"title": "Rule contract violation: message_empty",
	"severity": "medium",
	"description": "Gives an empty string for message.",
	"remediation": "Nothing to fix.",
	"target_type": "aws.example.widget",
	"references": [],
}

default fail := false

fail if input.tags.flag == "bad"

message := ""
