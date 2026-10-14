package yata.rules.test_policies.fail_missing.no_default

import rego.v1

metadata := {
	"title": "fail without default",
	"severity": "low",
	"description": "d",
	"remediation": "r",
	"target_type": "aws.example.widget",
	"references": [],
}

# Without input the body does not hold and there is no `default`, so `fail` is undefined.
fail if input.tags.flag == "bad"
