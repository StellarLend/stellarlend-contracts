import request from 'supertest';

jest.mock('../routes/lending.routes', () => {
  const express = require('express');
  return express.Router();
});

jest.mock('../routes/health.routes', () => {
  const express = require('express');
  return express.Router();
});

import app from '../app';
import { config } from '../config';

describe('CORS policy', () => {
  const originalOrigins = [...config.cors.origins];

  afterEach(() => {
    config.cors.origins = [...originalOrigins];
  });

  it('echoes an explicitly allowed origin', async () => {
    config.cors.origins = ['https://app.example'];

    const response = await request(app)
      .get('/api/cors-probe')
      .set('Origin', 'https://app.example');

    expect(response.headers['access-control-allow-origin']).toBe('https://app.example');
    expect(response.headers.vary).toContain('Origin');
  });

  it('does not reflect a disallowed origin and varies by origin', async () => {
    config.cors.origins = ['https://app.example'];

    const response = await request(app)
      .get('/api/cors-probe')
      .set('Origin', 'https://evil.example');

    expect(response.headers['access-control-allow-origin']).toBeUndefined();
    expect(response.headers.vary).toContain('Origin');
  });

  it('allows requests without an Origin header to behave normally', async () => {
    config.cors.origins = [];

    const response = await request(app).get('/api/cors-probe');

    expect(response.status).toBe(404);
    expect(response.headers['access-control-allow-origin']).toBeUndefined();
  });

  it('supports the configured development wildcard', async () => {
    config.cors.origins = ['*'];

    const response = await request(app)
      .get('/api/cors-probe')
      .set('Origin', 'https://any.example');

    expect(response.headers['access-control-allow-origin']).toBe('https://any.example');
  });

  it('handles allowed and rejected preflight requests', async () => {
    config.cors.origins = ['https://app.example'];

    const allowed = await request(app)
      .options('/api/cors-probe')
      .set('Origin', 'https://app.example')
      .set('Access-Control-Request-Method', 'GET');
    const rejected = await request(app)
      .options('/api/cors-probe')
      .set('Origin', 'https://evil.example')
      .set('Access-Control-Request-Method', 'GET');

    expect(allowed.headers['access-control-allow-origin']).toBe('https://app.example');
    expect(allowed.headers['access-control-allow-methods']).toContain('GET');
    expect(rejected.headers['access-control-allow-origin']).toBeUndefined();
    expect(rejected.headers.vary).toContain('Origin');
  });
});
