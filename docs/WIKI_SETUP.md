# GitHub Wiki Setup Guide

This document explains how to mirror the documentation to GitHub Wiki if desired.

## Current Setup

Documentation is currently maintained in the `docs/` directory of the main repository. This approach has several advantages:

 **Version controlled** - Documentation changes are tracked with code
 **PR workflow** - Documentation changes go through code review
 **Local access** - Easy to reference during development
 **Searchable** - GitHub's code search includes documentation
 **Single source of truth** - No sync issues between wiki and repo

## Option 1: Keep Documentation in Repository (Recommended)

**Advantages:**
- Already set up and working
- Documentation stays in sync with code
- PR reviews for documentation changes
- Full git history
- Easy to reference locally

**Access:**
- Main index: [docs/Home.md](Home.md)
- Direct GitHub view: `https://github.com/olafkfreund/cosmic-connect-desktop-app/tree/main/docs`

## Option 2: Mirror to GitHub Wiki

If you prefer to also maintain a GitHub Wiki, here's how to set it up:

### Step 1: Enable Wiki on GitHub

1. Go to repository Settings
2. Scroll to "Features" section
3. Enable "Wikis"

### Step 2: Clone Wiki Repository

```bash
# Clone the wiki repo (it's a separate git repository)
cd ~/Source/GitHub/
git clone git@github.com:olafkfreund/cosmic-connect-desktop-app.wiki.git
cd cosmic-connect-desktop-app.wiki
```

### Step 3: Copy Documentation

```bash
# Copy docs from main repo to wiki
cp -r ../cosmic-connect-desktop-app/docs/* .

# The Home.md file becomes the wiki homepage
# All other files can be accessed via wiki navigation
```

### Step 4: Adjust Links for Wiki

Wiki links need to be relative without the `.md` extension:

```bash
# In wiki, links should be:
[Architecture](architecture/Architecture)  # NOT Architecture.md

# Or for flat structure:
[Architecture](Architecture)
```

### Step 5: Commit and Push

```bash
git add .
git commit -m "Initial wiki setup from docs/ directory"
git push origin master
```

### Step 6: Keep in Sync

You'll need to manually sync changes from `docs/` to wiki, or set up automation.

**Manual sync script** (`sync-to-wiki.sh`):

```bash
#!/bin/bash
# Sync docs/ to wiki

REPO_DIR="../cosmic-connect-desktop-app"
WIKI_DIR="../cosmic-connect-desktop-app.wiki"

cd "$WIKI_DIR"
git pull

# Copy updated docs
rsync -av --delete "$REPO_DIR/docs/" . --exclude=.git

# Adjust markdown links for wiki (remove .md extensions)
find . -name "*.md" -type f -exec sed -i 's/\.md)/)/g' {} \;

git add .
git commit -m "Sync from docs/ $(date +%Y-%m-%d)"
git push

echo "Wiki synced successfully!"
```

## Option 3: Use GitHub Pages

Alternatively, you can set up GitHub Pages to render the documentation:

### Step 1: Enable GitHub Pages

1. Repository Settings → Pages
2. Source: Deploy from branch
3. Branch: `main`, Folder: `/docs`
4. Save

### Step 2: Add Jekyll Configuration (Optional)

Create `docs/_config.yml`:

```yaml
title: COSMIC Connect Documentation
description: Multi-platform device connectivity for COSMIC Desktop
theme: jekyll-theme-cayman
markdown: kramdown
```

### Step 3: Access Documentation

After deployment, documentation will be available at:
`https://olafkfreund.github.io/cosmic-connect-desktop-app/`

## Recommendation

**Stay with the current setup** (`docs/` directory in main repo) because:

1.  Documentation is version-controlled with code
2.  Changes go through PR review
3.  No sync issues
4.  Easy local access during development
5.  GitHub renders markdown nicely
6.  Can enable GitHub Pages if needed later

The GitHub Wiki is most useful for:
- User-facing documentation separate from code
- Community-editable content
- Non-technical documentation

For a technical project like COSMIC Connect, keeping docs in the repository is the best practice.

## Current Structure

```
docs/
├── Home.md                    # Main index
├── README.md                  # Directory overview
├── architecture/              # System design
├── development/               # Dev guides
├── project/                   # Project management
└── archive/                   # Historical docs
```

## Access URLs

- **Repository docs**: `https://github.com/olafkfreund/cosmic-connect-desktop-app/tree/main/docs`
- **Main index**: `https://github.com/olafkfreund/cosmic-connect-desktop-app/blob/main/docs/Home.md`
- **Architecture**: `https://github.com/olafkfreund/cosmic-connect-desktop-app/blob/main/docs/architecture/Architecture.md`

---

**Recommendation**: Keep using the current `docs/` directory structure. It's the best practice for technical projects.
