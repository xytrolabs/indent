"""
AXIS Discord Bot - PaxOS 3.4/Nightly
FSO Official Bot
"""

import discord
from discord.ext import commands
import asyncio
import os
from datetime import datetime
import logging
from dotenv import load_dotenv
from discord import app_commands

PROJECT_ROOT = os.path.dirname(os.path.abspath(__file__))


def load_project_environment():
    glo_path = os.path.join(PROJECT_ROOT, '.glo')
    env_path = os.path.join(PROJECT_ROOT, '.env')
    if not load_dotenv(dotenv_path=glo_path):
        load_dotenv(dotenv_path=env_path)


# Load project environment from .glo first, with .env as a legacy fallback.
load_project_environment()

import config

# Configure logging
logging.basicConfig(
    level=logging.INFO,
    format='%(asctime)s - %(name)s - %(levelname)s - %(message)s'
)
logger = logging.getLogger('AXIS')

# Bot configuration
intents = discord.Intents.all()
bot = commands.Bot(
    command_prefix=commands.when_mentioned_or(config.PREFIX),
    intents=intents,
    help_command=None  # We'll use custom help
)

# Version info
VERSION = config.VERSION

@bot.event
async def on_ready():
    """Called when the bot is ready"""
    logger.info(f'{bot.user} has connected to Discord!')
    logger.info(f'Running {VERSION}')

    # Set bot status
    activity = discord.Activity(
        type=discord.ActivityType.listening,
        name="FSO members"
    )
    await bot.change_presence(activity=activity)

    logger.info(f'Bot is in {len(bot.guilds)} guilds')
    print(f"""
    ╔═══════════════════════════════════════╗
    ║        AXIS BOT NOW ONLINE            ║
    ║      {VERSION}                        ║
    ║                                       ║
    ║  Bot: {bot.user.name}                 ║
    ║  Guilds: {len(bot.guilds)}            ║             
    ╚═══════════════════════════════════════╝
    """)

    try:
        registered_app_commands = len(bot.tree.get_commands())
        logger.info(f'Registered application commands before sync: {registered_app_commands}')
        logger.info(f'Registered prefix commands: {len(bot.commands)}')

        synced = await bot.tree.sync()
        print(f"Synced {len(synced)} global slash commands.")

        preferred_guild_id = getattr(config, 'IMMIGRATION_GUILD_ID', None) or getattr(config, 'GUILD_ID', None)
        if preferred_guild_id:
            guild_obj = discord.Object(id=int(preferred_guild_id))
            bot.tree.copy_global_to(guild=guild_obj)
            guild_synced = await bot.tree.sync(guild=guild_obj)
            print(f"Synced {len(guild_synced)} slash commands to guild {preferred_guild_id}.")
    except Exception as e:
        print(f"Failed to sync slash commands: {e}")

@bot.hybrid_command(name="changelog", description="Show the latest changelog.")
async def changelog(ctx):
    # Show the latest changelog
    changelog_path = os.path.join(os.path.dirname(__file__), 'CHANGELOG.txt')
    if os.path.exists(changelog_path):
        with open(changelog_path, 'r', encoding='utf-8') as f:
            changelog_text = f.read()
    else:
        changelog_text = "No changelog found."
    if getattr(ctx, 'interaction', None):
        if ctx.interaction.response.is_done():
            await ctx.interaction.followup.send(f"```\n{changelog_text}\n```", ephemeral=True)
        else:
            await ctx.interaction.response.send_message(f"```\n{changelog_text}\n```", ephemeral=True)
        return
    await ctx.send(f"```\n{changelog_text}\n```")

@bot.event
async def on_command_error(ctx, error):
    if hasattr(ctx.command, 'on_error'):
        # Defer to local error handlers when present
        return

    if isinstance(error, commands.CommandNotFound):
        return
    elif isinstance(error, commands.MissingPermissions):
        await ctx.send("❌ You don't have permission to use this command.")
    elif isinstance(error, commands.CheckFailure):
        await ctx.send("❌ You don't have permission to use this command.")
    elif isinstance(error, commands.MissingRequiredArgument):
        await ctx.send(f"❌ Missing required argument: {error.param.name}")
    elif isinstance(error, commands.CommandOnCooldown):
        await ctx.send(f"⏱️ This command is on cooldown. Try again in {error.retry_after:.2f}s")
    else:
        # Log full traceback for diagnostics
        logger.exception('Error in command %s', getattr(ctx.command, 'qualified_name', 'unknown'), exc_info=error)
        # Surface the error to the invoking channel to avoid silent failures
        embed = discord.Embed(
            title="❌ Something went wrong",
            color=config.COLOR_ERROR
        )
        if ctx.command:
            embed.add_field(name="Command", value=ctx.command.qualified_name, inline=False)
        embed.add_field(name="Error", value=f"```{error}``", inline=False)
        try:
            interaction = getattr(ctx, 'interaction', None)
            if interaction:
                if interaction.response.is_done():
                    await interaction.followup.send(embed=embed, ephemeral=True)
                else:
                    await interaction.response.send_message(embed=embed, ephemeral=True)
            else:
                await ctx.send(embed=embed)
        except Exception:
            try:
                await ctx.send("❌ Something went wrong while handling that command.")
            except Exception:
                pass

@bot.tree.error
async def on_app_command_error(interaction: discord.Interaction, error: app_commands.AppCommandError):
    logger.exception("App command error: %s", error)

    message = "❌ Slash command failed. Please try again or use the prefix command."
    if isinstance(error, app_commands.CheckFailure):
        message = "❌ You don't have permission to use this command."

    try:
        if interaction.response.is_done():
            await interaction.followup.send(message, ephemeral=True)
        else:
            await interaction.response.send_message(message, ephemeral=True)
    except Exception:
        pass

@bot.event
async def on_error(event_method, *args, **kwargs):
    # Catch-all for unhandled errors in other events; report to channel when possible.
    logger.exception('Unhandled error in %s', event_method)
    for arg in args:
        channel = getattr(arg, 'channel', None)
        if channel and hasattr(channel, 'send'):
            try:
                await channel.send("❌ An internal error occurred. The team has been notified.")
            except Exception:
                pass
            break

@bot.event
async def on_message(message):
    if message.author == bot.user:
        return

    moderation = bot.get_cog('Moderation')
    blacklisted_ids = set(getattr(moderation, 'blacklist', set())) if moderation else set()
    if message.author.id in blacklisted_ids:
        return

    if hasattr(message.author, 'roles'):
        if any(role.name == 'Blacklisted' for role in message.author.roles):
            return

    if message.mentions and bot.user in message.mentions:
        content_without_mention = message.content.replace(f'<@{bot.user.id}>', '').replace(f'<@!{bot.user.id}>', '').strip()
        if not content_without_mention:
            ctx = await bot.get_context(message)
            help_cmd = bot.get_cog('Help')
            if help_cmd:
                await help_cmd.help.callback(help_cmd, ctx)
            return

    # Always process prefix/hybrid commands for non-mention messages too.
    await bot.process_commands(message)

async def load_cogs():
    cogs_dir = os.path.join(os.path.dirname(__file__), 'cogs')
    cogs = []
    for filename in sorted(os.listdir(cogs_dir)):
        # Auto-load python modules in cogs/, skipping private or non-module files.
        if not filename.endswith('.py') or filename.startswith('_'):
            continue
        module_name = filename[:-3]
        cogs.append(f'cogs.{module_name}')

    for cog in cogs:
        try:
            await bot.load_extension(cog)
            logger.info(f'Loaded cog: {cog}')
        except Exception as e:
            logger.error(f'Failed to load cog {cog}: {e}')

async def main():
    async with bot:
        await load_cogs()
        token = os.getenv('DISCORD_TOKEN') or getattr(config, 'BOT_TOKEN', None)
        if not token:
            logger.error('No BOT_TOKEN found in config or DISCORD_TOKEN in .glo/.env!')
            return
        await bot.start(token)

if __name__ == '__main__':
    try:
        asyncio.run(main())
    except KeyboardInterrupt:
        logger.info('Bot shutdown requested by user')
    except Exception as e:
        logger.error(f'Fatal error: {e}')
