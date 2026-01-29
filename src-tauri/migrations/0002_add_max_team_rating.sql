-- Add max_team_rating column to event table
-- This migration adds a nullable REAL column to store the max team rating allowed/expected for the event.

ALTER TABLE event ADD COLUMN max_team_rating REAL;
