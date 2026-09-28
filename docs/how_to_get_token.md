# How to Generate Authentication Tokens

ForgeERP does not have a public login endpoint. Instead, you must generate your own JWT tokens for development and testing.

## 1. Token Generation Script

I have created a helper script at `scripts/get_token.py` that generates valid JWT tokens signed with the default development secret.

### Usage

Run the script from the project root:

```bash
# Generate a token with default settings (random tenant, admin role)
python3 scripts/get_token.py

# Generate a token for a specific tenant
python3 scripts/get_token.py --tenant "your-tenant-uuid"

# Generate a token with specific roles
python3 scripts/get_token.py --roles "warehouse,user"
```

### Output

The script will output the token details and the raw JWT string:

```text
Tenant ID: 4b72b683-deb8-4088-88e2-97d0668b2762
Roles: ['admin']
Secret: dev-secret
------------------------------------------------------------
eyJhbGciOiAiSFMyNTYi... <full token here>
```

## 2. Using the Token

Once you have the token, include it in the `Authorization` header of your HTTP requests.

### Example: Create Inventory Item

```bash
TOKEN="<paste-your-token-here>"

curl -X POST http://localhost:8080/inventory/items \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "Test Widget",
    "initial_quantity": 100
  }'
```

### Example: Check Identity

```bash
curl http://localhost:8080/whoami \
  -H "Authorization: Bearer $TOKEN"
```

> [!NOTE]
> Ensure your API server is running (`cargo run -p forgeerp-api`) before making requests.
