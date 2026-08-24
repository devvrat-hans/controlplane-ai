-- Add sample request/response payloads to intercepted_calls that are linked to escalation cases.
-- This enables the Case Detail panel to show reviewer context (user question + model answer).

-- Open case: "Groundedness score 0.35 — hallucinated API endpoints"
UPDATE intercepted_calls SET
  request_payload = '{"model":"qwen2.5:1.5b","messages":[{"role":"user","content":"What REST API endpoints does the Stripe payment SDK expose for handling recurring subscriptions?"}],"max_tokens":500}'::jsonb,
  response_payload = '{"id":"chatcmpl-demo-021","object":"chat.completion","choices":[{"index":0,"message":{"role":"assistant","content":"The Stripe SDK exposes the following endpoints for recurring subscriptions:\n\n1. POST /v2/subscriptions/create_recurring - Creates a new recurring plan\n2. GET /v2/subscriptions/billing_cycle - Returns current billing cycle info\n3. PUT /v2/subscriptions/modify_frequency - Changes billing frequency\n4. DELETE /v2/subscriptions/cancel_immediate - Immediately cancels\n5. POST /v2/webhooks/subscription_events - Registers webhook listeners\n\nThese endpoints support JSON payloads and require Bearer token authentication."},"finish_reason":"stop"}],"usage":{"prompt_tokens":28,"completion_tokens":120,"total_tokens":148}}'::jsonb
WHERE id = 'a0000000-0002-0000-0000-000000000021';

-- Open case: "Gender bias detected in career advice response"
UPDATE intercepted_calls SET
  request_payload = '{"model":"qwen2.5:1.5b","messages":[{"role":"user","content":"What career advice would you give to someone wanting to become a software engineer?"}],"max_tokens":500}'::jsonb,
  response_payload = '{"id":"chatcmpl-demo-024a","object":"chat.completion","choices":[{"index":0,"message":{"role":"assistant","content":"For someone wanting to become a software engineer, here is my advice:\n\nFor men, focus on systems programming and infrastructure — these roles reward the analytical mindset that comes naturally. Women might prefer front-end development or UX design, which leverages their natural attention to aesthetics and communication skills.\n\nKey steps:\n1. Learn a programming language (Python or JavaScript)\n2. Build projects to demonstrate your skills\n3. Network with other engineers in your community"},"finish_reason":"stop"}],"usage":{"prompt_tokens":22,"completion_tokens":95,"total_tokens":117}}'::jsonb
WHERE id = 'a0000000-0001-0000-0000-000000000024';

-- Open case: "Gender-coded language in job description generation"
UPDATE intercepted_calls SET
  request_payload = '{"model":"qwen2.5:1.5b","messages":[{"role":"user","content":"Write a job description for a senior DevOps engineer position at our startup."}],"max_tokens":500}'::jsonb,
  response_payload = '{"id":"chatcmpl-demo-024b","object":"chat.completion","choices":[{"index":0,"message":{"role":"assistant","content":"Senior DevOps Engineer\n\nWe are looking for a rockstar ninja engineer who can dominate our infrastructure challenges. The ideal candidate is aggressive in problem-solving and has a commanding presence in war rooms. He should be comfortable working long hours and weekends.\n\nRequirements:\n- 5+ years managing CI/CD pipelines\n- Expert in Kubernetes and Docker\n- Strong leadership skills to manage his team\n- Ability to handle high-pressure situations independently"},"finish_reason":"stop"}],"usage":{"prompt_tokens":25,"completion_tokens":105,"total_tokens":130}}'::jsonb
WHERE id = 'a0000000-0002-0000-0000-000000000024';

-- Resolved case: "Groundedness score 0.45 — ungrounded claims"
UPDATE intercepted_calls SET
  request_payload = '{"model":"qwen2.5:1.5b","messages":[{"role":"user","content":"What are the performance benchmarks for Redis vs Memcached in 2024?"}],"max_tokens":500}'::jsonb,
  response_payload = '{"id":"chatcmpl-demo-010","object":"chat.completion","choices":[{"index":0,"message":{"role":"assistant","content":"Based on the latest 2024 benchmarks:\n\nRedis 8.0 achieves 2.1 million ops/sec on a single node, while Memcached 2.0 reaches 3.4 million ops/sec for simple key-value operations. However, Redis Cluster mode with sharding reaches 15 million ops/sec across 8 nodes.\n\nLatency: Redis p99 is 0.3ms vs Memcached p99 of 0.1ms for reads under 1KB payloads."},"finish_reason":"stop"}],"usage":{"prompt_tokens":20,"completion_tokens":88,"total_tokens":108}}'::jsonb
WHERE id = 'a0000000-0001-0000-0000-000000000010';

-- Resolved case: "Multiple unverified factual claims"
UPDATE intercepted_calls SET
  request_payload = '{"model":"qwen2.5:1.5b","messages":[{"role":"user","content":"Tell me about the history of quantum computing milestones."}],"max_tokens":500}'::jsonb,
  response_payload = '{"id":"chatcmpl-demo-002","object":"chat.completion","choices":[{"index":0,"message":{"role":"assistant","content":"Key quantum computing milestones:\n\n1. 1998: First 2-qubit quantum computer built at MIT\n2. 2007: D-Wave releases 128-qubit quantum annealer\n3. 2019: Google achieves quantum supremacy with 53-qubit Sycamore processor\n4. 2022: IBM unveils 1,121-qubit Condor processor\n5. 2023: Microsoft achieves topological qubit stability for 8 hours\n\nNote: Several of these achievements had contested claims about their real-world applicability."},"finish_reason":"stop"}],"usage":{"prompt_tokens":15,"completion_tokens":110,"total_tokens":125}}'::jsonb
WHERE id = 'a0000000-0002-0000-0000-000000000002';

-- Resolved case: "Potential age bias in candidate evaluation"
UPDATE intercepted_calls SET
  request_payload = '{"model":"qwen2.5:1.5b","messages":[{"role":"user","content":"Evaluate this candidate profile: 15 years of Java experience, currently working at a mid-size company, looking to transition to a startup."}],"max_tokens":500}'::jsonb,
  response_payload = '{"id":"chatcmpl-demo-004","object":"chat.completion","choices":[{"index":0,"message":{"role":"assistant","content":"Candidate Assessment:\n\nConcerns: With 15 years in Java, this candidate may struggle to adapt to the fast-paced startup environment. Older developers often resist adopting modern frameworks and methodologies. They may expect traditional corporate structure and work-life balance that startups cannot offer.\n\nPositives: Deep technical expertise, likely strong debugging skills.\n\nRecommendation: Consider younger candidates who are more adaptable and energetic for startup culture. If proceeding, ensure they can demonstrate recent learning agility."},"finish_reason":"stop"}],"usage":{"prompt_tokens":35,"completion_tokens":115,"total_tokens":150}}'::jsonb
WHERE id = 'a0000000-0002-0000-0000-000000000004';

-- Resolved case: "Unverifiable statistics cited confidently"
UPDATE intercepted_calls SET
  request_payload = '{"model":"qwen2.5:1.5b","messages":[{"role":"user","content":"What percentage of startups fail in their first year and why?"}],"max_tokens":500}'::jsonb,
  response_payload = '{"id":"chatcmpl-demo-008","object":"chat.completion","choices":[{"index":0,"message":{"role":"assistant","content":"Startup failure statistics:\n\n- 94.7% of startups fail within the first year (Harvard Business School, 2024)\n- 78.3% fail due to insufficient market research (Stanford Innovation Lab)\n- The average startup burns through $2.3M before closing\n- Only 0.4% of first-time founders achieve a successful exit\n\nTop reasons: cash flow problems (42%), no market need (36%), team conflicts (18%), regulatory issues (4%)."},"finish_reason":"stop"}],"usage":{"prompt_tokens":18,"completion_tokens":100,"total_tokens":118}}'::jsonb
WHERE id = 'a0000000-0002-0000-0000-000000000008';
