package yata.rules.eval_policies.bad.fail_conflict

import rego.v1

metadata := {
	"title": "Rule contract violation: fail_conflict",
	"severity": "low",
	"description": "Gives two different values for fail once there is a widget.",
	"remediation": "None.",
	"target_type": "aws.example.widget",
	"references": [],
}

default fail := false

fail if input.id

fail := false if input.id
