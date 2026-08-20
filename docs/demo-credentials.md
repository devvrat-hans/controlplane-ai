# Demo Credentials

These accounts are pre-seeded in the database by migration `009_seed_demo_data.sql`.

## User Accounts

| Email | Password | Role | Permissions |
|-------|----------|------|-------------|
| `admin@controlplane.test` | `Demo#Admin2026` | admin | Full access: policies, escalations, audit, settings |
| `reviewer@controlplane.test` | `Demo#Reviewer2026` | reviewer | Can resolve escalations, view audit trail |
| `viewer@controlplane.test` | `Demo#Viewer2026` | viewer | Read-only dashboard access |

## Demo Applications

| App ID | Name | Description |
|--------|------|-------------|
| `10000000-...-000000000001` | ChatBot-Prod | Production customer-facing chatbot |
| `10000000-...-000000000002` | Agent-Internal | Internal agentic workflow assistant |
| `10000000-...-000000000003` | RAG-Customer-Support | RAG-powered customer support system |

## Authentication Flow

1. **Login**: POST to the dashboard at `/login` with email + password
2. **Token**: JWT is returned and stored in `sessionStorage`
3. **API calls**: Token is sent as `Authorization: Bearer <token>`
4. **Demo bypass**: If no token is present, API allows anonymous access

## Generating Tokens Manually

```bash
# Using the API
curl -X POST http://localhost:8080/api/v1/auth/login \
  -H "Content-Type: application/json" \
  -d '{"email": "admin@controlplane.test", "password": "Demo#Admin2026"}'

# Response: { "token": "eyJ...", "user": { ... } }
```

## JWT Secret

- Default: `demo-secret-key-change-in-prod`
- Override via environment: `JWT_SECRET=your-production-secret`
- Algorithm: HS256
- Expiry: 24 hours

## Security Notes

- These credentials are for demonstration only
- Passwords use placeholder hashes in the database (demo auth bypasses bcrypt)
- In production: use proper password hashing (argon2id), rotate JWT secrets, add refresh tokens
