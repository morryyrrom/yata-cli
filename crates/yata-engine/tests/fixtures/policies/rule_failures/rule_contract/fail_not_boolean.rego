package yata.rules.test_policies.rule_contract.fail_not_boolean

import rego.v1

metadata := {
	"title": "Rule contract violation: fail_not_boolean",
	"severity": "medium",
	"description": "Gives a string for fail for a widget flagged bad.",
	"remediation": "Nothing to fix.",
	"target_type": "aws.example.widget",
	"references": [],
}

default fail := false

fail := "yes" if input.tags.flag == "bad"
