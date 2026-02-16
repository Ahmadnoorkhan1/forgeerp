# ForgeERP Documentation Guide

**Welcome!** This is your entry point to all ForgeERP documentation. Everything is organized by topic to help you quickly find what you need.

---

## 📚 Documentation Map

### I'm New to ForgeERP - Start Here! 🚀

1. **[README.md](README.md)** - Project overview and features (10 min read)
2. **[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)** - System architecture overview (15 min read)
3. **Choose your next step below** based on what you want to do

---

## 📋 Core Feature Documentation

### Authentication & User Management

| Document | Purpose | Read Time |
|----------|---------|-----------|
| **[AUTHENTICATION.md](AUTHENTICATION.md)** | 🔐 Complete auth system, login/register flow, JWT tokens, Arc fix | 20 min |
| **[REGISTRATION.md](REGISTRATION.md)** | 👤 User registration features, implementation, API endpoints | 25 min |
| **[PERSISTENT_STORAGE.md](PERSISTENT_STORAGE.md)** | 💾 Database setup, PostgreSQL/Redis config, data persistence | 20 min |

### Testing & Verification

| Document | Purpose | Read Time |
|----------|---------|-----------|
| **[TESTING.md](TESTING.md)** | ✅ Comprehensive test procedures, debugging tips, troubleshooting | 30 min |

---

## 🗂️ By Use Case

### "I want to get the system running"
→ Follow: [PERSISTENT_STORAGE.md](PERSISTENT_STORAGE.md) - Quick Start section
→ Time: 5-10 minutes

### "I want to test everything works"
→ Follow: [TESTING.md](TESTING.md) - Quick Test section
→ Time: 5 minutes

### "I want to understand the architecture"
→ Read: [AUTHENTICATION.md](AUTHENTICATION.md) - Architecture section
→ Then: [REGISTRATION.md](REGISTRATION.md) - Architecture section  
→ Time: 30 minutes

### "I want to implement the backend"
→ Read: [AUTHENTICATION.md](AUTHENTICATION.md) - Implementation Details
→ Follow: [TESTING.md](TESTING.md) - Step-by-step
→ Time: 1-2 hours

### "I need to set up production"
→ Follow: [PERSISTENT_STORAGE.md](PERSISTENT_STORAGE.md) - Detailed Setup
→ Then: [AUTHENTICATION.md](AUTHENTICATION.md) - Phase 2 section
→ Time: 2-4 hours

### "Something is broken, help!"
→ See: [TESTING.md](TESTING.md) - Troubleshooting section
→ Or: Individual doc troubleshooting sections
→ Time: Depends on issue

---

## 📁 File Organization

The documentation is organized into a clean structure:

```
forgeerp/
├── README.md                          ← Project overview & features
├── MIGRATIONS.md                      ← Database migrations
│
├── 📚 CORE DOCUMENTATION
├── AUTHENTICATION.md                  ← Auth system, login, JWT
├── REGISTRATION.md                    ← User registration features
├── PERSISTENT_STORAGE.md              ← Database configuration
├── TESTING.md                         ← Test procedures
├── DOCUMENTATION.md                   ← Navigation guide (you are here)
│
└── docs/                              ← Additional technical docs
    ├── ARCHITECTURE.md                ← System architecture
    ├── PROJECTION_IMPLEMENTATION.md   ← Projection details
    ├── CORS_FIX_SUMMARY.md           ← CORS configuration
    ├── PUBLIC_TENANTS_ENDPOINT.md    ← Tenant endpoints
    ├── INVENTORY_VS_USERS_COMPARISON.md ← Pattern comparison
    ├── WHY_NO_DATA_IN_TABLES.md      ← Debugging guide
    └── how_to_get_token.md           ← Token generation
```

---

## 🔍 Quick Reference

### Common Commands

```bash
# Start development environment
docker compose up -d              # Start PostgreSQL + Redis
docker compose run --rm migrate   # Apply migrations
RUST_LOG=debug cargo run -p forgeerp-api  # Run API with logging

# With persistent storage
USE_PERSISTENT_STORES=true cargo run -p forgeerp-api --features redis

# Test API
curl -X POST http://localhost:8080/auth/register \
  -H "Content-Type: application/json" \
  -d '{"username":"test","email":"test@example.com","password":"pass123"}'
```

### Key Concepts

| Concept | Explanation | Doc |
|---------|-------------|-----|
| **Arc<Mutex<T>>** | Shared mutable state pattern in Rust | [AUTHENTICATION.md](AUTHENTICATION.md#key-concepts) |
| **Event Sourcing** | Append-only event log as source of truth | [AUTHENTICATION.md](AUTHENTICATION.md#architecture) |
| **CQRS** | Separate command and query responsibilities | [AUTHENTICATION.md](AUTHENTICATION.md#architecture) |
| **JWT Token** | JSON Web Token for stateless auth | [AUTHENTICATION.md](AUTHENTICATION.md#jwt-token-contents) |
| **Projection** | Read model built from events | [docs/PROJECTION_IMPLEMENTATION.md](docs/PROJECTION_IMPLEMENTATION.md) |

---

## 📖 Reading Paths

### Path 1: Quick Start (15 minutes)
Best for: "I just want to get it running"

1. Read: [PERSISTENT_STORAGE.md](PERSISTENT_STORAGE.md) - Quick Start section (3 min)
2. Follow: Steps 1-3 (10 min)
3. Test: [TESTING.md](TESTING.md) - Quick Test (2 min)

### Path 2: Understanding (45 minutes)
Best for: "I want to understand how this works"

1. Read: [AUTHENTICATION.md](AUTHENTICATION.md) - The Problem & Fix (5 min)
2. Read: [AUTHENTICATION.md](AUTHENTICATION.md) - Architecture (10 min)
3. Read: [AUTHENTICATION.md](AUTHENTICATION.md) - How It Works (10 min)
4. Read: [AUTHENTICATION.md](AUTHENTICATION.md) - Visual Diagrams (10 min)
5. Test: [TESTING.md](TESTING.md) - Quick Test (5 min)

### Path 3: Implementation (2 hours)
Best for: "I need to implement the backend"

1. Read: [AUTHENTICATION.md](AUTHENTICATION.md) - All sections (40 min)
2. Read: [REGISTRATION.md](REGISTRATION.md) - All sections (30 min)
3. Follow: [PERSISTENT_STORAGE.md](PERSISTENT_STORAGE.md) - Detailed Setup (30 min)
4. Follow: [TESTING.md](TESTING.md) - In-Memory Mode Tests (15 min)
5. Follow: [TESTING.md](TESTING.md) - Persistent Storage Tests (15 min)

### Path 4: Production Deployment (4 hours)
Best for: "I need to deploy this to production"

1. Complete: Path 3 above (2 hours)
2. Follow: [PERSISTENT_STORAGE.md](PERSISTENT_STORAGE.md) - Production setup (30 min)
3. Follow: [AUTHENTICATION.md](AUTHENTICATION.md) - Phase 2 Production section (30 min)
4. Follow: [TESTING.md](TESTING.md) - Full test suite (45 min)
5. Deploy: Following your organization's procedures (30 min)

---

## 🆘 Troubleshooting Entry Points

### Problem: "Can't log in"
→ See: [TESTING.md](TESTING.md#problem-login-fails-after-register)

### Problem: "Registration returns 500 error"
→ See: [AUTHENTICATION.md](AUTHENTICATION.md#troubleshooting)
→ See: [TESTING.md](TESTING.md#troubleshooting)

### Problem: "Data not in database"
→ See: [PERSISTENT_STORAGE.md](PERSISTENT_STORAGE.md#troubleshooting)

### Problem: "API won't start"
→ See: [TESTING.md](TESTING.md#troubleshooting)

### Problem: "Tests are failing"
→ See: [TESTING.md](TESTING.md#troubleshooting-checklist)

### Problem: "I don't know where to start"
→ Start here: This document! 👈

---

## 📊 Documentation Status

| Document | Status | Topic | Priority |
|----------|--------|-------|----------|
| AUTHENTICATION.md | ✅ Complete | Auth system | 🔴 Critical |
| REGISTRATION.md | ✅ Complete | User registration | 🔴 Critical |
| PERSISTENT_STORAGE.md | ✅ Complete | Database setup | 🟠 High |
| TESTING.md | ✅ Complete | Testing | 🟠 High |
| DOCUMENTATION.md | ✅ Complete | Navigation | 🟠 High |
| docs/ARCHITECTURE.md | ✅ Complete | Architecture | 🟡 Medium |
| docs/PROJECTION_IMPLEMENTATION.md | ✅ Exists | Projections | 🟡 Medium |

---

## 🎯 Next Steps

### Immediate (Next 15 minutes)
- [ ] Choose a reading path above
- [ ] Start with the first document
- [ ] Run the quick test
- [ ] Verify it works ✅

### Short Term (This week)
- [ ] Complete all authentication setup
- [ ] Understand event sourcing pattern  
- [ ] Review Phase 2 production setup
- [ ] Plan migration timeline

### Medium Term (This month)
- [ ] Implement Phase 2 (PostgreSQL)
- [ ] Deploy to staging
- [ ] Run full test suite
- [ ] Performance testing

### Long Term (This quarter)
- [ ] Implement Phase 3 (Full event sourcing)
- [ ] Deploy to production
- [ ] Monitor and optimize
- [ ] Plan next features

---

## 💡 Pro Tips

1. **Use Ctrl+F to search** - Each document is comprehensive and searchable
2. **Follow the quick tests first** - Verify things work before diving deep
3. **Read troubleshooting sections** - They explain what can go wrong and why
4. **Check the Table of Contents** - Most docs have one at the top
5. **Use the diagrams** - Visual learners will benefit from ASCII diagrams
6. **Copy the commands** - All curl/bash commands are ready to paste
7. **Cross-reference links** - Jump between docs to see related info

---

## 📞 Support

If you can't find what you need:

1. **Check the Troubleshooting section** - Most issues are covered
2. **Read the full document** - Use Ctrl+F to search
3. **Check the Table of Contents** - Each doc has one
4. **Look in the FAQ section** - Each doc has Q&A
5. **Check related docs** - Links at the top usually guide you

---

## 🎓 Learning Resources

### For Event Sourcing Concepts
→ See: [AUTHENTICATION.md - Architecture section](AUTHENTICATION.md#architecture)
→ See: [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)
→ See: [REGISTRATION.md](REGISTRATION.md) - How it works section

### For Rust Patterns
→ See: [AUTHENTICATION.md - Key Concepts](AUTHENTICATION.md#key-concepts)
→ See: Arc<Mutex<T>> explanation in AUTHENTICATION.md

### For API Endpoints
→ See: [AUTHENTICATION.md - API Endpoints](AUTHENTICATION.md#api-endpoints)
→ See: [REGISTRATION.md - API Specification](REGISTRATION.md)

### For Database
→ See: [PERSISTENT_STORAGE.md - Architecture](PERSISTENT_STORAGE.md#architecture)
→ See: [docs/PROJECTION_IMPLEMENTATION.md](docs/PROJECTION_IMPLEMENTATION.md)

---

## ✅ Verification Checklist

You've mastered the documentation when you can:

- [ ] Explain what Arc<Mutex<T>> does
- [ ] Describe the registration flow end-to-end
- [ ] Run a test and verify logs
- [ ] Set up PostgreSQL and migrations
- [ ] Troubleshoot a failing login
- [ ] Understand event sourcing principles
- [ ] Deploy to production confidently

---

## 📝 Document Summary

### AUTHENTICATION.md (40 pages)
Covers the entire authentication system including the Arc fix, JWT, architecture, and three phases of implementation.

**Key sections**:
- The Problem & Fix (Arc<Mutex> issue)
- Architecture & Key Concepts
- How Registration & Login Work
- Visual Diagrams
- Implementation Details
- Testing & Verification
- Troubleshooting

### REGISTRATION.md (Complete)
Covers user registration features, implementation, and API endpoints.

**Key sections**:
- Feature Overview
- User Registration Flow
- API Endpoints
- Frontend Components
- Implementation Steps

### PERSISTENT_STORAGE.md (35 pages)
Complete guide for setting up PostgreSQL and Redis for persistent data storage.

**Key sections**:
- Quick Start (3 steps)
- Detailed Setup
- Environment Configuration
- Architecture (In-Memory vs Persistent)
- Verification & Testing
- Troubleshooting

### TESTING.md (40 pages)
Comprehensive testing guide with procedures, debugging tips, and troubleshooting.

**Key sections**:
- Quick Test (5 minutes)
- In-Memory Mode Tests
- Persistent Storage Tests
- Projection Tests
- Debugging Tips
- Troubleshooting
- Success Criteria

---

## 🚀 You're Ready!

Now that you understand the documentation structure, pick your path above and get started. Good luck! 🎉

**Any questions?** Check the troubleshooting sections in the relevant documents.

**Stuck?** Revisit the appropriate reading path above.

**Ready to contribute?** Start with the Understanding path, then read the implementation docs.

Happy coding! 🙌
