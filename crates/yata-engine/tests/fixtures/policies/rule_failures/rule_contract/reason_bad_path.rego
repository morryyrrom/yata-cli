package yata.rules.test_policies.rule_contract.reason_bad_path

import rego.v1

metadata := {
	"title": "Rule contract violation: reason_bad_path",
	"severity": "medium",
	"description": "Gives a path that is not a JSON Pointer.",
	"remediation": "Nothing to fix.",
	"target_type": "aws.example.widget",
	"references": [],
}

default fail := false

fail if input.tags.flag == "bad"

reason := {"path": "tags"}
