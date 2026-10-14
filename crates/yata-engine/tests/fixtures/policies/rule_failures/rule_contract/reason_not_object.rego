package yata.rules.test_policies.rule_contract.reason_not_object

import rego.v1

metadata := {
	"title": "Rule contract violation: reason_not_object",
	"severity": "medium",
	"description": "Gives an array for reason.",
	"remediation": "Nothing to fix.",
	"target_type": "aws.example.widget",
	"references": [],
}

default fail := false

fail if input.tags.flag == "bad"

reason := ["a"]
