package yata.rules.test_policies.rule_contract.fail_conflict

import rego.v1

metadata := {
	"title": "Rule contract violation: fail_conflict",
	"severity": "medium",
	"description": "Gives true and false for a widget flagged bad.",
	"remediation": "Nothing to fix.",
	"target_type": "aws.example.widget",
	"references": [],
}

default fail := false

fail if input.tags.flag == "bad"

fail := false if input.tags.flag == "bad"
