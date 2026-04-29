# Vaults API Documentation

## Overview

The Vaults API provides endpoints for creating, managing, and retrieving vault information in the StellarLend platform. Vaults represent time-locked financial arrangements with specific amounts, destinations, and optional milestones.

## Base URL

```
https://api.stellarlend.com/api
```

## Authentication

All vault endpoints require authentication using Bearer tokens in the Authorization header:
```
Authorization: Bearer <your-jwt-token>
```

## Endpoints

### Create Vault

**POST** `/vaults`

Creates a new vault with the specified parameters.

#### Request Body

```json
{
  "amount": 1000,
  "endTimestamp": 1735689600,
  "destination": "test-destination",
  "milestones": [
    {
      "timestamp": 1735686000,
      "description": "First milestone",
      "amount": 500
    }
  ],
  "metadata": {
    "key": "value"
  },
  "tags": ["tag1", "tag2"]
}
```

#### Parameters

| Parameter | Type | Required | Description | Constraints |
|-----------|------|----------|-------------|-------------|
| `amount` | integer | Yes | Vault amount in smallest units | 1 - 9,007,199,254,740,991 |
| `endTimestamp` | integer | Yes | Unix timestamp when vault expires | Must be 60s - 1 year from now |
| `destination` | string | Yes | Destination identifier | 1-255 chars, alphanumeric/hyphens/underscores |
| `milestones` | array | No | Array of milestone objects | Max 100 items |
| `metadata` | object | No | Additional vault metadata | Max 10KB when serialized |
| `tags` | array | No | Vault tags for categorization | Max 20 items, max 50 chars each |

#### Milestone Object

| Parameter | Type | Required | Description | Constraints |
|-----------|------|----------|-------------|-------------|
| `timestamp` | integer | Yes | Milestone timestamp | Must be positive |
| `description` | string | No | Milestone description | Max 500 characters |
| `amount` | number | No | Milestone amount | Must be non-negative |

#### Response

**201 Created**
```json
{
  "success": true,
  "data": {
    "id": "vault_1703123456789_abc123def",
    "amount": 1000,
    "endTimestamp": 1735689600,
    "destination": "test-destination",
    "milestones": [...],
    "metadata": {...},
    "tags": ["tag1", "tag2"],
    "createdAt": "2023-12-21T10:30:45.123Z",
    "status": "active"
  },
  "message": "Vault created successfully"
}
```

**400 Bad Request** - Validation errors
```json
{
  "success": false,
  "message": "Amount must be an integer between 1 and 9007199254740991"
}
```

### Get All Vaults

**GET** `/vaults`

Retrieves a list of all vaults for the authenticated user.

#### Query Parameters

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `page` | integer | No | Page number (default: 1) |
| `limit` | integer | No | Items per page (default: 20, max: 100) |
| `status` | string | No | Filter by vault status |
| `tag` | string | No | Filter by tag |

#### Response

**200 OK**
```json
{
  "success": true,
  "data": [
    {
      "id": "vault_1703123456789_abc123def",
      "amount": 1000,
      "endTimestamp": 1735689600,
      "destination": "test-destination",
      "status": "active",
      "createdAt": "2023-12-21T10:30:45.123Z"
    }
  ],
  "pagination": {
    "page": 1,
    "limit": 20,
    "total": 1,
    "pages": 1
  },
  "message": "Vaults retrieved successfully"
}
```

### Get Vault by ID

**GET** `/vaults/{id}`

Retrieves a specific vault by its ID.

#### Path Parameters

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `id` | string | Yes | Vault ID |

#### Response

**200 OK**
```json
{
  "success": true,
  "data": {
    "id": "vault_1703123456789_abc123def",
    "amount": 1000,
    "endTimestamp": 1735689600,
    "destination": "test-destination",
    "milestones": [...],
    "metadata": {...},
    "tags": ["tag1", "tag2"],
    "createdAt": "2023-12-21T10:30:45.123Z",
    "status": "active"
  },
  "message": "Vault retrieved successfully"
}
```

**404 Not Found**
```json
{
  "success": false,
  "message": "Vault not found"
}
```

### Update Vault

**PUT** `/vaults/{id}`

Updates an existing vault with new values.

#### Path Parameters

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `id` | string | Yes | Vault ID |

#### Request Body

All parameters are optional. Only provided fields will be updated.

```json
{
  "amount": 2000,
  "destination": "updated-destination",
  "endTimestamp": 1735776000
}
```

#### Response

**200 OK**
```json
{
  "success": true,
  "data": {
    "id": "vault_1703123456789_abc123def",
    "amount": 2000,
    "destination": "updated-destination",
    "endTimestamp": 1735776000,
    "updatedAt": "2023-12-22T10:30:45.123Z"
  },
  "message": "Vault updated successfully"
}
```

**404 Not Found**
```json
{
  "success": false,
  "message": "Vault not found"
}
```

### Delete Vault

**DELETE** `/vaults/{id}`

Deletes a vault permanently.

#### Path Parameters

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `id` | string | Yes | Vault ID |

#### Response

**200 OK**
```json
{
  "success": true,
  "message": "Vault deleted successfully"
}
```

**404 Not Found**
```json
{
  "success": false,
  "message": "Vault not found"
}
```

## Validation Rules

### Amount Validation
- Must be an integer between 1 and 9,007,199,254,740,991 (MAX_SAFE_INTEGER)
- Cannot be negative or zero
- Must be provided as a number, not a string

### End Timestamp Validation
- Must be a Unix timestamp (seconds since epoch)
- Must be at least 60 seconds in the future
- Must not be more than 1 year (31,536,000 seconds) in the future
- Cannot be in the past

### Destination Validation
- Must be 1-255 characters long
- Can only contain: letters, numbers, hyphens (-), and underscores (_)
- Cannot contain spaces or special characters
- Cannot be empty or null

### Milestones Validation
- Maximum of 100 milestones per vault
- Each milestone must have a positive timestamp
- Milestone timestamps must be in chronological order
- Optional description (max 500 characters)
- Optional amount (must be non-negative)

### Metadata Validation
- Must be a valid JSON object
- Serialized size cannot exceed 10KB
- Cannot be null or a primitive type

### Tags Validation
- Maximum of 20 tags per vault
- Each tag must be 1-50 characters
- Can only contain: letters, numbers, hyphens (-), and underscores (_)
- Cannot contain spaces or special characters

## Security Considerations

### Payload Size Limits
- Maximum request payload size: 1MB
- Maximum metadata size: 10KB
- These limits prevent DoS attacks from oversized payloads

### Rate Limiting
- All endpoints are subject to rate limiting
- Default: 100 requests per 15-minute window per IP
- Exceeding limits returns 429 Too Many Requests

### Input Validation
- All inputs are strictly validated before processing
- Malicious payloads are rejected with appropriate error messages
- SQL injection and XSS protection through input sanitization

## Error Handling

### Standard Error Response Format

```json
{
  "success": false,
  "message": "Human-readable error description"
}
```

### Common HTTP Status Codes

| Status Code | Description | Common Causes |
|-------------|-------------|---------------|
| 200 | OK | Successful request |
| 201 | Created | Vault created successfully |
| 400 | Bad Request | Validation errors, malformed JSON |
| 401 | Unauthorized | Missing or invalid authentication |
| 404 | Not Found | Vault does not exist |
| 429 | Too Many Requests | Rate limit exceeded |
| 500 | Internal Server Error | Unexpected server error |

### Validation Error Examples

```json
{
  "success": false,
  "message": "Amount must be an integer between 1 and 9007199254740991"
}
```

```json
{
  "success": false,
  "message": "End timestamp must be at least 60 seconds in the future"
}
```

```json
{
  "success": false,
  "message": "Destination can only contain alphanumeric characters, hyphens, and underscores"
}
```

## SDK Examples

### JavaScript/TypeScript

```typescript
import { StellarLendAPI } from '@stellarlend/sdk';

const api = new StellarLendAPI({ apiKey: 'your-api-key' });

// Create a vault
const vault = await api.vaults.create({
  amount: 1000,
  endTimestamp: Math.floor(Date.now() / 1000) + 3600,
  destination: 'my-vault',
  milestones: [
    { timestamp: Math.floor(Date.now() / 1000) + 1800, description: 'Mid-point' }
  ]
});

// Get vault by ID
const retrievedVault = await api.vaults.get(vault.id);

// Update vault
await api.vaults.update(vault.id, { amount: 2000 });

// Delete vault
await api.vaults.delete(vault.id);
```

### cURL Examples

```bash
# Create vault
curl -X POST https://api.stellarlend.com/api/vaults \
  -H "Authorization: Bearer your-token" \
  -H "Content-Type: application/json" \
  -d '{
    "amount": 1000,
    "endTimestamp": 1735689600,
    "destination": "test-vault"
  }'

# Get vault
curl -X GET https://api.stellarlend.com/api/vaults/vault_123 \
  -H "Authorization: Bearer your-token"

# Update vault
curl -X PUT https://api.stellarlend.com/api/vaults/vault_123 \
  -H "Authorization: Bearer your-token" \
  -H "Content-Type: application/json" \
  -d '{"amount": 2000}'

# Delete vault
curl -X DELETE https://api.stellarlend.com/api/vaults/vault_123 \
  -H "Authorization: Bearer your-token"
```

## Testing

The vault validation includes comprehensive test coverage for:

- **Boundary conditions**: Minimum/maximum values for all numeric fields
- **Type validation**: Ensures correct data types for all fields
- **Format validation**: String patterns and character restrictions
- **Array validation**: Size limits and item structure validation
- **Security testing**: Payload size limits and malicious input handling
- **Error consistency**: Stable error message formatting

### Running Tests

```bash
# Run all vault tests
npm test -- --testPathPattern=vault

# Run with coverage
npm test -- --coverage --testPathPattern=vault

# Run specific test file
npm test vaults.test.ts
```

## Rate Limits and Quotas

| Endpoint | Rate Limit | Description |
|----------|------------|-------------|
| POST /vaults | 10/minute | Vault creation |
| GET /vaults | 100/minute | Vault listing |
| GET /vaults/{id} | 200/minute | Vault retrieval |
| PUT /vaults/{id} | 50/minute | Vault updates |
| DELETE /vaults/{id} | 20/minute | Vault deletion |

## Version History

| Version | Date | Changes |
|---------|------|---------|
| v1.0.0 | 2023-12-21 | Initial vault API release |
| v1.1.0 | 2024-01-15 | Added metadata and tags support |
| v1.2.0 | 2024-02-01 | Enhanced validation rules |
