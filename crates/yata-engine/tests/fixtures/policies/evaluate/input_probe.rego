package yata.rules.eval_policies.input_probe

import rego.v1

metadata := {
	"title": "Reports the shape of its input",
	"severity": "info",
	"description": "Fails every widget with details about `input` and `data` in `actual`.",
	"remediation": "None.",
	"target_type": "aws.example.widget",
	"related_types": ["aws.example.gadget"],
	"references": [],
}

# Every field is reported in `actual`, to check the shape of `input` and `data`.
default fail := false

fail if input.id

message := "probe"

reason := {"actual": {
	"input_keys": sort([k | some k, _ in input]),
	"input_id": input.id,
	"account_id": data.yata.context.account.id,
	"regions": data.yata.context.regions,
	"context_keys": sort([k | some k, _ in data.yata.context]),
	"example_keys": sort([k | some k, _ in data.aws.example]),
	"service_keys": sort([k | some k, _ in data.aws]),
	"widget_order": [sprintf("%s@%s", [w.id, w.region]) | some w in data.aws.example.widget],
	"gadget_count": count(data.aws.example.gadget),
}}
