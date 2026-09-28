#!/usr/bin/env python3
import hmac
import hashlib
import base64
import json
import time
import argparse
import uuid
import os

def base64url_encode(data):
    if isinstance(data, str):
        data = data.encode('utf-8')
    return base64.urlsafe_b64encode(data).rstrip(b'=').decode('utf-8')

def create_jwt(secret, tenant_id, roles):
    header = {
        "alg": "HS256",
        "typ": "JWT"
    }
    
    now = int(time.time())
    claims = {
        "sub": str(uuid.uuid4()),  # Random principal ID
        "tenant_id": tenant_id,
        "roles": roles,
        "iat": now,
        "exp": now + 3600  # 1 hour expiration
    }
    
    encoded_header = base64url_encode(json.dumps(header))
    encoded_claims = base64url_encode(json.dumps(claims))
    
    message = f"{encoded_header}.{encoded_claims}"
    signature = hmac.new(
        secret.encode('utf-8'),
        message.encode('utf-8'),
        hashlib.sha256
    ).digest()
    
    encoded_signature = base64url_encode(signature)
    
    return f"{message}.{encoded_signature}"

def main():
    parser = argparse.ArgumentParser(description='Generate a JWT for ForgeERP')
    parser.add_argument('--secret', default=os.environ.get('JWT_SECRET', 'dev-secret'), help='JWT secret (default: dev-secret or JWT_SECRET env)')
    parser.add_argument('--tenant', help='Tenant ID (default: generates a new UUID)')
    parser.add_argument('--roles', default='admin', help='Comma-separated roles (default: admin)')
    
    args = parser.parse_args()
    
    tenant_id = args.tenant if args.tenant else str(uuid.uuid4())
    roles = args.roles.split(',')
    
    token = create_jwt(args.secret, tenant_id, roles)
    
    print(f"Tenant ID: {tenant_id}")
    print(f"Roles: {roles}")
    print(f"Secret: {args.secret}")
    print("-" * 60)
    print(token)

if __name__ == "__main__":
    main()
