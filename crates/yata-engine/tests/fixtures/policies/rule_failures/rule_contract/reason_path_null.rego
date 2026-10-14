package yata.rules.test_policies.rule_contract.reason_path_null

import rego.v1

metadata := {
	"title": "Rule contract violation: reason_path_null",
	"severity": "medium",
	"description": "Gives null for path.",
	"remediation": "Nothing to fix.",
	"target_type": "aws.example.widget",
	"references": [],
}

default fail := false

fail if input.tags.flag == "bad"

reason := {"path": null}
