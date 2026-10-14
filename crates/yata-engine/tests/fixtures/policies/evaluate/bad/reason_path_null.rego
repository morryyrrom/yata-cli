package yata.rules.eval_policies.bad.reason_path_null

import rego.v1

metadata := {
	"title": "Rule contract violation: reason_path_null",
	"severity": "low",
	"description": "Gives null for path when the widget is named bad.",
	"remediation": "None.",
	"target_type": "aws.example.widget",
	"references": [],
}

default fail := false

fail if input.name == "bad"

reason := {"path": null}
